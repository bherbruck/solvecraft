// SPDX-License-Identifier: Apache-2.0
//! STEP codec backend and encoder.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::archive;
use crate::dialect::refuse_alternate_encoding;
use crate::options::{StepSchema, StepWriteOptions};
use crate::parse;
use crate::reader;

/// STEP encoder with per-export header options.
#[derive(Debug, Clone, Default)]
pub struct StepCodec {
    /// Header metadata and deterministic writer options.
    pub options: StepWriteOptions,
}

impl EncoderBackend for StepCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(StepSchema::TARGETS, Some(2));

    /// Synthesis-only encoder. An off-catalog STEP source cannot be reproduced
    /// because every emitted schema stamps object-identifier arcs.
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        crate::writer::target::plan(self, input, &target)
    }
}

impl CodecBackend for StepCodec {
    const FORMAT: FormatId = FormatId::new(crate::dialect::FORMAT);

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let view = prefix;
        let prefix = prefix.window();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(prefix.len()),
            "detect STEP trivia",
        )?;
        if starts_with_step_magic(prefix) {
            return Ok(Confidence::High);
        }
        if archive::has_root_marker(ctx, view)? {
            return Ok(Confidence::Medium);
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(prefix.len()),
            "detect STEP HDF5",
        )?;
        if is_part26_hdf5(prefix) {
            return Ok(Confidence::Medium);
        }
        let xml_bytes = cadmpeg_core::decode::u64_from_index(prefix.len().min(4096));
        ctx.charge_work(xml_bytes * 6, "detect STEP Part 28 XML")?;
        if is_part28_xml(prefix) {
            return Ok(Confidence::Medium);
        }
        ctx.charge_work(xml_bytes * 5, "detect STEP business-object XML")?;
        if is_ap242_bo_model_xml(prefix) {
            return Ok(Confidence::Medium);
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(prefix.len().min(4)),
            "detect STEP ZIP",
        )?;
        Ok(if archive::has_zip_magic(prefix) {
            Confidence::Low
        } else {
            Confidence::No
        })
    }

    fn inspect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes)
            || (!starts_with_step_magic(bytes)
                && cadmpeg_container::ArchiveSnapshot::has_footer(ctx, bytes)?)
        {
            return inspect_zip(ctx, root);
        }
        let inspected = inspect_exchange(self, ctx, root)?;
        Ok(ContainerSummary::classified(
            DialectLayers::of(inspected.matched),
            cadmpeg_ir::ContainerKind::Iso10303ClearText,
            inspected.entries,
            inspected.losses,
            inspected.notes,
        ))
    }

    fn decode_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<Decoded, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes)
            || (!starts_with_step_magic(bytes)
                && cadmpeg_container::ArchiveSnapshot::has_footer(ctx, bytes)?)
        {
            return decode_zip(ctx, root);
        }
        refuse_alternate_encoding(bytes)?;
        if self.detect_impl(ctx, root)? == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        reader::decode(bytes, ctx, reader::Packaging::Bare)
    }
}

/// One Part 21 exchange as `inspect` sees it: its single classification and
/// the container facts every packaging of it reports.
struct InspectedExchange {
    matched: DialectMatch,
    entries: Vec<ContainerEntry>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
}

fn insert_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: &'static str,
    value: String,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(key.len()),
        "step_inspect_attribute_key",
    )?;
    let key = ctx.copy_retained_text(key, "step_inspect_attribute_key")?;
    ctx.insert_btree_map(attributes, key, value, "step_inspect_attributes")?;
    Ok(())
}

struct CountItem<'a> {
    name: &'a str,
    count: usize,
}

impl fmt::Display for CountItem<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}:{}", self.name, self.count)
    }
}

/// Deep semantic analysis of a STEP exchange, exposed as container inspect.
///
/// Runs the semantic decode path to populate `unknown_entities` and related
/// attributes. Not a cheap syntactic census; see `docs/formats/step-inspect.md`.
fn inspect_exchange(
    codec: &StepCodec,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<InspectedExchange, CodecError> {
    let bytes = root.window();
    refuse_alternate_encoding(bytes)?;
    if codec.detect_impl(ctx, root)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let (mut exchange, diagnostics) = parse::parse_with_context(bytes, ctx)?;
    inspect_parsed_exchange(bytes, ctx, &mut exchange, &diagnostics)
}

fn inspect_parsed_exchange(
    bytes: &[u8],
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    exchange: &mut parse::Exchange,
    diagnostics: &[parse::ParseDiagnostic],
) -> Result<InspectedExchange, CodecError> {
    let reader::AnalyzedExchange {
        decoded,
        matched,
        opaque_offsets,
    } = reader::analyze_exchange(bytes, exchange, diagnostics, ctx)?;
    let mut entries = Vec::new();
    ctx.push_vec(
        &mut entries,
        ContainerEntry {
            name: ctx.copy_retained_text("HEADER", "step_inspect_section_label")?,
            role: ContainerRole::Metadata,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes: BTreeMap::default(),
        },
        "step_inspect_entries",
    )?;
    if !exchange.anchors().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "anchor_count",
            ctx.format_retained(
                format_args!("{}", exchange.anchors().len()),
                "step_inspect_attribute_count",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.copy_retained_text("ANCHOR", "step_inspect_section_label")?,
                role: ContainerRole::InFileAnchors,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    if !exchange.references().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "external_count",
            ctx.format_retained(
                format_args!("{}", exchange.references().len()),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(
            ctx,
            &mut attributes,
            "external_uris",
            ctx.join_display_retained(
                exchange.references().iter().map(|entry| entry.uri.as_str()),
                ",",
                "step_inspect_external_uris",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.copy_retained_text("REFERENCE", "step_inspect_section_label")?,
                role: ContainerRole::ExternalReferences,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    let mut section_counts = ctx.alloc_filled(
        exchange.data().len(),
        (0_usize, BTreeMap::<&str, usize>::new()),
        "step_inspect_data_counts",
    )?;
    let section_lookup_work = u64::from(
        exchange
            .data()
            .len()
            .checked_ilog2()
            .map_or(0, |bits| bits + 1),
    );
    for record in exchange.records().values() {
        ctx.charge_work(section_lookup_work, "step_inspect_data_owner")?;
        let index = exchange
            .data()
            .partition_point(|section| section.span.end <= record.span.start);
        let (entity_count, counts) = section_counts
            .get_mut(index)
            .ok_or_else(|| CodecError::malformed("STEP instance has no DATA section"))?;
        *entity_count += 1;
        if !opaque_offsets.contains(&record.span.start) {
            continue;
        }
        for partial in &record.partials {
            let name = partial.name.as_str();
            ctx.admit_btree_entry(counts, &name, "step_inspect_unknown_counts")?;
            match counts.entry(name) {
                Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                Entry::Vacant(entry) => {
                    entry.insert(1);
                }
            }
        }
    }
    for (index, (entity_count, counts)) in section_counts.into_iter().enumerate() {
        let unknown = ctx.join_display_retained(
            counts
                .iter()
                .map(|(&name, &count)| CountItem { name, count }),
            ",",
            "step_inspect_unknown_entities",
        )?;
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "entity_count",
            ctx.format_retained(
                format_args!("{entity_count}"),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(ctx, &mut attributes, "unknown_entities", unknown)?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx
                    .format_retained(format_args!("DATA[{index}]"), "step_inspect_section_name")?,
                role: ContainerRole::EntityRecords,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    let external_dependencies = decoded.body.notes.iter().filter(|note| {
        note.starts_with("external document ") || note.starts_with("external source ")
    });
    let dependency_count = external_dependencies.clone().count();
    if dependency_count > 0 {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "dependency_count",
            ctx.format_retained(
                format_args!("{dependency_count}"),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(
            ctx,
            &mut attributes,
            "dependencies",
            ctx.join_display_retained(
                external_dependencies.map(String::as_str),
                ",",
                "step_inspect_dependency_text",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx
                    .copy_retained_text("EXTERNAL_DEPENDENCIES", "step_inspect_section_label")?,
                role: ContainerRole::ExternalReferences,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    for (index, signature) in exchange.signatures().iter().enumerate() {
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: if index == 0 {
                    ctx.copy_retained_text("SIGNATURE", "step_inspect_signature_name")?
                } else {
                    ctx.format_retained(
                        format_args!("SIGNATURE[{index}]"),
                        "step_inspect_signature_indexed_name",
                    )?
                },
                role: ContainerRole::Signature,
                storage: EntryStorage::verbatim(
                    VerbatimLabel::None,
                    cadmpeg_core::decode::u64_from_index(signature.len()),
                ),
                attributes: BTreeMap::default(),
            },
            "step_inspect_entries",
        )?;
    }
    let identifiers = exchange.joined_schema_identifiers(ctx)?;
    let schema = if identifiers.is_empty() {
        "unspecified".into()
    } else {
        identifiers
    };
    let dialect = matched.dialect();
    let mut notes = Vec::new();
    ctx.push_vec(
        &mut notes,
        ctx.format_retained(
            format_args!("schema {schema}; dialect {dialect}"),
            "step_inspect_schema_note",
        )?,
        "step_codec_notes",
    )?;
    for diagnostic in diagnostics {
        let note = ctx.format_retained(
            format_args!("{}", diagnostic.message),
            "step_inspect_diagnostic_copy",
        )?;
        ctx.push_vec(&mut notes, note, "step_codec_notes")?;
    }
    Ok(InspectedExchange {
        matched,
        entries,
        losses: decoded.body.losses,
        notes,
    })
}

fn starts_with_step_magic(bytes: &[u8]) -> bool {
    starts_with_exchange_magic(bytes, b"ISO-10303-21;", false)
        || draft_exchange_offset(bytes).is_some()
}

pub(crate) fn draft_exchange_offset(bytes: &[u8]) -> Option<usize> {
    if starts_with_exchange_magic(bytes, b"STEP;", true) {
        return Some(0);
    }
    // A leading identifier ends at a comment delimiter. A complete comment
    // followed by the draft magic supplies a known exchange boundary.
    let prefix_end = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
        .count();
    (bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && matches!(bytes.get(prefix_end..prefix_end + 2), Some(b"/*" | b"!*"))
        && starts_with_exchange_magic(&bytes[prefix_end..], b"STEP;", true))
    .then_some(prefix_end)
}

fn starts_with_exchange_magic(bytes: &[u8], magic: &[u8], draft: bool) -> bool {
    let mut at = 0;
    loop {
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
        {
            at += 1;
        }
        if bytes.get(at..at + 2) == Some(b"/*") || (draft && bytes.get(at..at + 2) == Some(b"!*")) {
            let close = if bytes[at] == b'/' { b"*/" } else { b"*!" };
            at += 2;
            let Some(relative_end) = bytes[at..].windows(2).position(|window| window == close)
            else {
                return false;
            };
            at += relative_end + 2;
            continue;
        }
        if bytes
            .get(at..at + 3)
            .is_some_and(|prefix| prefix == b"\\N\\" || prefix == b"\\F\\")
        {
            at += 3;
            continue;
        }
        break;
    }
    for &expected_byte in magic {
        while bytes.get(at).is_some_and(u8::is_ascii_control) {
            at += 1;
        }
        if !bytes
            .get(at)
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&expected_byte))
        {
            return false;
        }
        at += 1;
    }
    true
}

fn inspect_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let archive::OpenedRoot {
        archive,
        view: root_view,
        data_start: root_data_offset,
    } = archive::open_root(ctx, root)?;
    let root_bytes = root_view.window();
    refuse_alternate_encoding(root_bytes)?;
    if StepCodec::default().detect_impl(ctx, root_view)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let (mut exchange, diagnostics) = parse::parse_with_context(root_bytes, ctx)?;
    let resource_notes = archive::root_reference_notes(ctx, &archive, &exchange);
    let mut inspected = inspect_parsed_exchange(root_bytes, ctx, &mut exchange, &diagnostics)?;
    let resource_notes = resource_notes?;
    let entry_count = archive.entries().len();
    let logical_entries = ctx.join_display_retained(
        inspected.entries.iter().map(|entry| entry.name.as_str()),
        ",",
        "step_inspect_logical_sections",
    )?;
    let mut entries = archive.container_entries(ctx, archive::classify_entry)?;
    if let Some(root_entry) = entries
        .iter_mut()
        .find(|entry| entry.name == archive::ROOT_NAME)
    {
        insert_attribute(
            ctx,
            &mut root_entry.attributes,
            "logical_sections",
            logical_entries,
        )?;
    }
    let mut notes = Vec::new();
    {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("root {}", archive::ROOT_NAME),
            "step_codec_notes",
            "step_codec_root_note",
        )?;
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("archive entries={entry_count}; root data offset={root_data_offset}"),
            "step_codec_notes",
            "step_codec_archive_note",
        )
    }?;
    ctx.extend_vec(
        &mut notes,
        std::mem::take(&mut inspected.notes),
        "step_codec_notes",
    )?;
    let mut losses = std::mem::take(&mut inspected.losses);
    ctx.extend_vec(
        &mut losses,
        archive::recovery::losses(ctx, &archive)?,
        "STEP ZIP combined losses",
    )?;
    ctx.extend_vec(&mut notes, resource_notes, "step_codec_notes")?;
    // ZIP packaging is a container fact, not an identity axis: the
    // `ISO-10303.p21` root carries the FILE_SCHEMA that classifies the
    // document, so the root's own match is this summary's match.
    Ok(ContainerSummary::classified(
        DialectLayers::of(inspected.matched),
        cadmpeg_ir::ContainerKind::Iso10303Zip,
        entries,
        losses,
        notes,
    ))
}

fn decode_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<Decoded, CodecError> {
    let archive::OpenedRoot {
        archive,
        view: root_view,
        data_start: root_data_offset,
    } = archive::open_root(ctx, root)?;
    let (exchange, diagnostics) = parse::parse_with_context(root_view.window(), ctx)?;
    let resource_notes = archive::root_reference_notes(ctx, &archive, &exchange)?;
    let entry_count = archive.entries().len();
    let mut decoded = reader::decode_exchange(
        root_view.window(),
        exchange,
        &diagnostics,
        ctx,
        reader::Packaging::Zip {
            entry_count,
            root_data_offset,
        },
    )?;
    ctx.push_formatted_retained(
        &mut decoded.body.notes,
        format_args!(
            "container root {}; archive entries={entry_count}",
            archive::ROOT_NAME
        ),
        "step_codec_notes",
        "step_codec_container_note",
    )?;
    ctx.extend_vec(&mut decoded.body.notes, resource_notes, "step_codec_notes")?;
    ctx.extend_vec(
        &mut decoded.body.losses,
        archive::recovery::losses(ctx, &archive)?,
        "STEP ZIP combined losses",
    )?;
    archive::recovery::retain(ctx, &archive, root, &mut decoded)?;
    Ok(decoded)
}

pub(crate) fn is_part26_hdf5(bytes: &[u8]) -> bool {
    const SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";
    if bytes.starts_with(SIGNATURE) {
        return true;
    }

    let mut offset = 512;
    while offset < bytes.len() {
        if bytes[offset..].starts_with(SIGNATURE) {
            return true;
        }
        let Some(next) = offset.checked_mul(2) else {
            break;
        };
        offset = next;
    }
    false
}

pub(crate) fn is_part28_xml(bytes: &[u8]) -> bool {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some((name, attributes)) = xml_root_start_tag(bytes) else {
        return false;
    };
    let local_name = name
        .iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |separator| &name[separator + 1..]);
    if local_name.eq_ignore_ascii_case(b"iso_10303_28")
        || ascii_starts_with(local_name, b"iso_10303_28_")
    {
        return true;
    }

    // A configured UOS can use a local name other than the document marker.
    // Its governing-schema namespace varies by AP, but the Part 28 common
    // namespace remains the bounded admission marker. Schema selection and
    // the derived XML Schema remain caller inputs.
    PART28_COMMON_NAMESPACES
        .iter()
        .any(|namespace| has_namespace_value(attributes, namespace))
}

const PART28_COMMON_NAMESPACES: [&[u8]; 3] = [
    b"urn:oid:1.0.10303.28.2.1.1",
    b"urn:iso:std:iso:10303:-28:ed-2:tech:XMLschema:common",
    b"urn:iso.org:standard:10303:part(28):version(2):xmlschema:common",
];

fn ascii_starts_with(value: &[u8], prefix: &[u8]) -> bool {
    value.len() >= prefix.len()
        && value[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(value, prefix)| value.eq_ignore_ascii_case(prefix))
}

fn xml_root_start_tag(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut cursor = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    loop {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'<') {
            return None;
        }
        if bytes.get(cursor + 1) == Some(&b'?') {
            let end = bytes
                .get(cursor + 2..)?
                .windows(2)
                .position(|window| window == b"?>")?
                + cursor
                + 2;
            cursor = end + 2;
            continue;
        }
        if bytes.get(cursor + 1..cursor + 4) == Some(b"!--") {
            let end = bytes
                .get(cursor + 4..)?
                .windows(3)
                .position(|window| window == b"-->")?
                + cursor
                + 4;
            cursor = end + 3;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'!') {
            let end = bytes
                .get(cursor + 2..)?
                .iter()
                .position(|byte| *byte == b'>')?
                + cursor
                + 2;
            cursor = end + 1;
            continue;
        }
        break;
    }

    let tag_end = find_xml_tag_end(bytes, cursor + 1)?;
    let mut name_end = cursor + 1;
    while bytes
        .get(name_end)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'/' && *byte != b'>')
    {
        name_end += 1;
    }
    if name_end == cursor + 1 {
        return None;
    }
    Some((&bytes[cursor + 1..name_end], &bytes[name_end..tag_end]))
}

fn find_xml_tag_end(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    let mut quote = None;
    while let Some(byte) = bytes.get(cursor).copied() {
        match quote {
            Some(delimiter) if byte == delimiter => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(cursor),
            None => {}
        }
        cursor += 1;
    }
    None
}

fn has_namespace_value(attributes: &[u8], expected: &[u8]) -> bool {
    xml_attribute_value(attributes, |name, value| {
        (name == b"xmlns" || name.starts_with(b"xmlns:")) && value == expected
    })
    .is_some()
}

fn xml_attribute_value(
    attributes: &[u8],
    mut matches: impl FnMut(&[u8], &[u8]) -> bool,
) -> Option<&[u8]> {
    let mut cursor = 0;
    while cursor < attributes.len() {
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if attributes.get(cursor).is_none_or(|byte| *byte == b'/') {
            break;
        }
        let name_start = cursor;
        while attributes
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'/')
        {
            cursor += 1;
        }
        let name = &attributes[name_start..cursor];
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if attributes.get(cursor) != Some(&b'=') {
            return None;
        }
        cursor += 1;
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let delimiter = *attributes.get(cursor)?;
        if delimiter != b'\'' && delimiter != b'"' {
            return None;
        }
        cursor += 1;
        let value_start = cursor;
        while attributes
            .get(cursor)
            .is_some_and(|byte| *byte != delimiter)
        {
            cursor += 1;
        }
        let value = &attributes[value_start..cursor];
        cursor += 1;
        if matches(name, value) {
            return Some(value);
        }
    }
    None
}

pub(crate) fn is_ap242_bo_model_xml(bytes: &[u8]) -> bool {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some((name, attributes)) = xml_root_start_tag(bytes) else {
        return false;
    };
    let local_name = name
        .iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |separator| &name[separator + 1..]);
    // BM-03: the published namespace must be bound on the Uos document
    // element. Text, comments, schemaLocation values, and local names do not
    // identify the alternate encoding.
    local_name == b"Uos"
        && BO_MODEL_NAMESPACES
            .iter()
            .any(|namespace| has_namespace_value(attributes, namespace))
}

const BO_MODEL_NAMESPACES: [&[u8]; 2] = [
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-1/tech/xml-schema/bo_model",
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model",
];

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, InspectOptions, ResourceDimension,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

    use super::{insert_attribute, starts_with_step_magic, StepCodec};

    const INSPECTION_TEXT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";

    #[test]
    fn inspection_count_text_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_attribute_count");
    }

    #[test]
    fn inspection_attribute_key_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_attribute_key");
    }

    #[test]
    fn inspection_owned_attribute_value_is_not_charged_twice() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(
            13 + 11 * std::mem::size_of::<(String, String)>()
                + 16 * std::mem::size_of::<usize>()
                + 2 * std::mem::align_of::<String>(),
        )
        .expect("attribute node layout");
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut attributes = std::collections::BTreeMap::new();
            let value = ctx
                .copy_retained_text("1", "step_test_attribute_value")
                .expect("one-byte value");
            insert_attribute(ctx, &mut attributes, "entity_count", value)
                .expect("twelve-byte key fits once");
            assert_eq!(attributes["entity_count"], "1");
        });
    }

    #[test]
    fn inspect_attribute_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits selected policy");
        let mut attributes = std::collections::BTreeMap::new();
        assert!(matches!(
            insert_attribute(&ctx, &mut attributes, "unknown_entities", "ITEM:1".into()),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_inspect_attributes"
        ));
    }

    fn inspect_text_refuses(source: &[u8], operation: &str) {
        let mut limit = 0u64;
        for _ in 0..512 {
            let (mut exchange, diagnostics) =
                crate::test_support::with_service_context(source, crate::parse::parse_inner)
                    .expect("valid inspect source");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits retained policy");
            match super::inspect_parsed_exchange(source, &ctx, &mut exchange, &diagnostics) {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("retained requirement fits u64");
                    assert!(next > limit, "retained limit must advance");
                    limit = next;
                }
                Ok(_) => panic!("inspect completed before {operation}"),
                Err(error) => panic!("inspect did not reach {operation}: {error}"),
            }
        }
        panic!("inspect never reached {operation}");
    }

    #[test]
    fn inspect_external_uris_refuse_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_external_uris");
    }

    #[test]
    fn inspect_unknown_entities_refuse_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_unknown_entities");
    }

    #[test]
    fn inspect_schema_note_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_schema_note");
    }

    #[test]
    fn inspect_diagnostic_copy_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;9');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_diagnostic_copy");
    }

    #[test]
    fn inspect_dependency_text_refuses_retained_limit() {
        let source = include_bytes!("../tests/fixtures/ap242_external_documents.p21");
        inspect_text_refuses(source, "step_inspect_dependency_text");
    }

    #[test]
    fn inspect_unknown_count_entries_refuse_collection_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let mut limit = 0u64;
        for _ in 0..512 {
            let (mut exchange, diagnostics) =
                crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
                    .expect("valid inspect source");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                .expect("root fits collection policy");
            match super::inspect_parsed_exchange(SOURCE, &ctx, &mut exchange, &diagnostics) {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == "step_inspect_unknown_counts" =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("collection requirement fits u64");
                    assert!(next > limit, "collection limit must advance");
                    limit = next;
                }
                Ok(_) => panic!("inspect completed before count admission"),
                Err(error) => panic!("inspect did not reach count admission: {error}"),
            }
        }
        panic!("inspect never reached count admission");
    }

    fn zip_text_refuses(operation: &str, inspect: bool) {
        use std::io::Write;

        const ROOT: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "ISO-10303.p21",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start ZIP root");
        writer.write_all(ROOT).expect("write ZIP root");
        let bytes = writer.finish().expect("finish STEP ZIP").into_inner();

        let mut limit = 0u64;
        for _ in 0..1024 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let result = crate::test_support::with_policy_context(&bytes, &policy, |bytes, ctx| {
                let root = cadmpeg_core::decode::View::over_retained(bytes);
                if inspect {
                    super::inspect_zip(ctx, root).map(|_| ())
                } else {
                    super::decode_zip(ctx, root).map(|_| ())
                }
            });
            match result {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("retained requirement fits u64");
                    assert!(next > limit, "retained limit must advance");
                    limit = next;
                }
                Ok(()) => panic!("ZIP operation completed before {operation}"),
                Err(error) => panic!("ZIP operation did not reach {operation}: {error}"),
            }
        }
        panic!("ZIP operation never reached {operation}");
    }

    #[test]
    fn inspect_zip_logical_sections_refuse_retained_limit() {
        zip_text_refuses("step_inspect_logical_sections", true);
    }

    #[test]
    fn inspection_section_name_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_section_name");
    }

    #[test]
    fn inspection_signature_name_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA= ENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA= ENDSEC;";
        inspect_text_refuses(SOURCE, "step_inspect_signature_indexed_name");
    }

    #[test]
    fn zip_inspection_archive_note_refuses_retained_limit() {
        zip_text_refuses("step_codec_archive_note", true);
    }

    #[test]
    fn zip_decode_container_note_refuses_retained_limit() {
        zip_text_refuses("step_codec_container_note", false);
    }

    #[test]
    fn detects_magic_after_ignored_controls_and_inside_token() {
        let source = b"\0 /* leading comment */ \\N\\ ISO-10303-\n21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let codec = StepCodec::default();

        assert!(starts_with_step_magic(source));
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, source),
            Confidence::High
        );
        codec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode Part 21 with ignored framing octets");

        let with_bom = [b"\xEF\xBB\xBF".as_slice(), source].concat();
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, &with_bom),
            Confidence::No
        );
        assert!(!starts_with_step_magic(b"/* incomplete ISO-10303-21;"));
    }

    #[test]
    fn inspect_accepts_noncanonical_complex_partial_order_and_reports_a_note() {
        let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
        let summary = StepCodec::default()
            .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
            .expect("inspection describes recoverable source order");

        assert!(summary
            .notes
            .iter()
            .any(|note| note.contains("complex partial records are not alphabetical")));
    }
    #[test]
    fn inspection_counts_data_populations_by_source_extent() {
        let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('first',('AP242'));#90=FIRST_ITEM();#3=SECOND_ITEM(#90);ENDSEC;DATA('second',('AP242'));#1=THIRD_ITEM();ENDSEC;END-ISO-10303-21;";
        let summary = StepCodec::default()
            .inspect(&mut Cursor::new(source), &InspectOptions::default())
            .expect("named DATA sections retain their own populations");
        let first = summary
            .entries
            .iter()
            .find(|entry| entry.name == "DATA[0]")
            .expect("first population");
        let second = summary
            .entries
            .iter()
            .find(|entry| entry.name == "DATA[1]")
            .expect("second population");
        assert_eq!(first.attributes["entity_count"], "2");
        assert_eq!(
            first.attributes["unknown_entities"],
            "FIRST_ITEM:1,SECOND_ITEM:1"
        );
        assert_eq!(second.attributes["entity_count"], "1");
        assert_eq!(second.attributes["unknown_entities"], "THIRD_ITEM:1");
    }
}
