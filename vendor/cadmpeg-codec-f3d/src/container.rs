// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Scan and classify Fusion `.f3d` and `.f3z` ZIP containers.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. Model geometry is selected from Design body-to-blob
//! bindings by [`crate::decode`]. [`select_history_brep`] independently locates
//! the stream whose header declares a history partition. Compatibility helpers
//! expose unique or legacy carrier sets for metadata reporting; model decode
//! uses the typed Design body-map catalog.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ContainerSummary;

use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::{acis_header, asm_header};

use crate::dialect::F3dDialect;
use crate::manifest;

pub(crate) fn copy_summary_entries(
    ctx: &DecodeContext<'_>,
    entries: &[ContainerEntry],
) -> Result<Vec<ContainerEntry>, CodecError> {
    let mut copied = ctx.collection_vec(entries.len(), "copy F3D summary entries")?;
    for entry in entries {
        let mut attributes = BTreeMap::new();
        for (key, value) in &entry.attributes {
            ctx.admit_btree_entry(&attributes, key, "copy F3D summary attributes")?;
            attributes.insert(
                ctx.copy_retained_text(key, "copy F3D summary attribute key")?,
                ctx.copy_retained_text(value, "copy F3D summary attribute value")?,
            );
        }
        copied.push(ContainerEntry {
            name: ctx.copy_retained_text(&entry.name, "copy F3D summary entry name")?,
            role: entry.role,
            storage: entry.storage.clone(),
            attributes,
        });
    }
    Ok(copied)
}

/// Write-path local cap for nested Protein rewriting (`patch_protein_appearances`).
/// Decode opens nested archives through `ArchiveSnapshot` / `begin_expand`, so
/// session `ResourceLimits` bind there instead of these constants.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Write-path per-entry inflate cap for `read_entry_bounded`.
pub(crate) const MAX_INFLATED_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

/// The f3d marker substrings used for confident detection from a byte prefix
/// (ZIP local file headers store entry names in cleartext near the start).
pub(crate) const DETECT_MARKERS: &[&[u8]] = &[
    b"Breps.BlobParts",
    b"FusionAssetName",
    b"FusionDocType",
    b".smbh",
];
/// Marker names that distinguish a multi-document F3Z archive from a generic ZIP.
pub(crate) const F3Z_DETECT_MARKERS: &[&[u8]] =
    &[b"Manifest.json", b"DesignDescription.json", b".f3d"];

pub(crate) fn read_entry_bounded(
    entry: &mut impl Read,
    declared_size: u64,
    name: &str,
) -> Result<Vec<u8>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    if declared_size > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    let mut bytes = Vec::new();
    let mut limited = Read::take(entry, MAX_INFLATED_ENTRY_BYTES + 1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = limited.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        ctx.extend_retained_bytes(&mut bytes, &chunk[..read], "F3D entry allocation")?;
    }
    if u64_from_index(bytes.len()) > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    Ok(bytes)
}

/// Classify an entry by its name using the spec's naming families ([§1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#1-container-layer), [§6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#6-geometry-carriers)).
pub(crate) fn classify(name: &str) -> ContainerRole {
    if name.ends_with('/') {
        return ContainerRole::Directory;
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smbh"))
    {
        ContainerRole::BrepSmbh
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smb"))
    {
        ContainerRole::BrepSmb
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sat") || ext.eq_ignore_ascii_case("smt"))
    {
        ContainerRole::BrepText
    } else if name.ends_with(".protein") {
        ContainerRole::ProteinAssets
    } else if name.ends_with(".paramesh") {
        ContainerRole::Paramesh
    } else if name.ends_with(".dsgcfg") || name.ends_with(".dsgcfgrule") {
        ContainerRole::DesignConfig
    } else if base == "Manifest.dat" {
        ContainerRole::Manifest
    } else if base == "MetaStream.dat" {
        ContainerRole::Metastream
    } else if base == "BulkStream.dat" {
        ContainerRole::Bulkstream
    } else if base == "Properties.dat" {
        ContainerRole::Properties
    } else if name.contains("Previews/") {
        ContainerRole::Preview
    } else if name.contains("Images.BlobParts") {
        ContainerRole::Image
    } else if name.contains("OGS.BlobFolder/") {
        ContainerRole::OgsCache
    } else {
        ContainerRole::Other
    }
}

/// Owned kernel framing for one BREP stream.
#[derive(Debug, Clone)]
pub(crate) enum KernelFraming {
    /// Autodesk Shape Manager binary framing.
    Asm {
        /// Parsed ASM header.
        header: BinaryHeader,
        /// Exact byte boundary between solved records and construction history.
        solved_record_limit: Option<usize>,
    },
    /// Spatial ACIS binary framing.
    Acis(BinaryHeader),
    /// Text kernel framing without a binary reference width.
    Text {
        /// Encoding-independent header metadata.
        header: KernelHeader,
        /// Family selected by the text terminator.
        terminator: cadmpeg_asm::sat::Terminator,
    },
}

impl KernelFraming {
    /// Borrow this owned framing as the shared dialect-classification input.
    pub(crate) fn as_header_ref(&self) -> cadmpeg_asm::dialect::KernelHeaderRef<'_> {
        match self {
            Self::Asm { header, .. } => cadmpeg_asm::dialect::KernelHeaderRef::Asm(header),
            Self::Acis(header) => cadmpeg_asm::dialect::KernelHeaderRef::Acis(header),
            Self::Text { header, terminator } => match terminator {
                cadmpeg_asm::sat::Terminator::Asm => {
                    cadmpeg_asm::dialect::KernelHeaderRef::TextAsm(header)
                }
                cadmpeg_asm::sat::Terminator::Acis => {
                    cadmpeg_asm::dialect::KernelHeaderRef::TextAcis(header)
                }
            },
        }
    }

    /// The ASM solved-record boundary, when present.
    pub(crate) fn solved_record_limit(&self) -> Option<usize> {
        match self {
            Self::Asm {
                solved_record_limit,
                ..
            } => *solved_record_limit,
            Self::Acis(_) | Self::Text { .. } => None,
        }
    }

    /// Metadata retained for ASM binary and text model reports.
    pub(crate) fn model_metadata(&self) -> Option<&KernelHeader> {
        match self {
            Self::Asm { header, .. } => Some(&header.metadata),
            Self::Text { header, .. } => Some(header),
            Self::Acis(_) => None,
        }
    }

    /// Return the header only when ASM framing owns it.
    pub(crate) fn asm_header(&self) -> Option<&BinaryHeader> {
        match self {
            Self::Asm { header, .. } => Some(header),
            Self::Acis(_) | Self::Text { .. } => None,
        }
    }
}

/// One decoded BREP stream's header facts, kept for the summary and decode
/// metadata.
#[derive(Debug, Clone)]
pub(crate) struct BrepFacts {
    /// Entry name.
    pub(crate) name: String,
    /// Uncompressed byte length.
    pub(crate) uncompressed_len: u64,
    /// Parsed ASM or ACIS framing, when either header matched.
    pub(crate) kernel: Option<KernelFraming>,
    /// SHA-256 (lowercase hex) of the decompressed stream.
    pub(crate) sha256: Sha256Digest,
}

/// The manifest-level kind of a scanned Fusion archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum F3dContainerKind {
    /// One F3D document whose manifests select one Design asset folder.
    Document {
        /// Exact archive folder of the Design asset.
        design_asset_folder: String,
        /// Dialect classified from the document manifest.
        matched: DialectMatch,
    },
    /// An outer F3Z archive whose `.f3d` members each carry their own
    /// manifests and Design asset.
    MultiDocument {
        /// Dialect classified from the outer archive manifests.
        matched: DialectMatch,
    },
}

impl F3dContainerKind {
    /// Primary dialect selected by this container kind's discriminants.
    pub(crate) fn dialect(&self) -> &DialectMatch {
        match self {
            Self::Document { matched, .. } | Self::MultiDocument { matched } => matched,
        }
    }
}

/// The full result of reading a Fusion ZIP: the entry list plus decoded BREP
/// facts. Shared by `inspect` and `decode`.
///
/// The `'a` lifetime is the session's root address space: stored entries are
/// views borrowing the root without copying, and compressed entries are arena-backed views the
/// platform expander produced, so both live for the decode's duration.
pub(crate) struct ContainerScan<'a> {
    /// Complete source archive retained for byte-exact native replay.
    pub(crate) source_image: &'a [u8],
    /// Enumerated entries with classification.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Decoded BREP stream facts, in archive order.
    pub(crate) breps: Vec<BrepFacts>,
    /// Each text B-rep's one admitted parse, shared by model and report paths.
    pub(crate) text_breps: std::collections::HashMap<String, TextBrepFraming>,
    /// Whether this ZIP is one F3D document or an outer F3Z archive.
    pub(crate) kind: F3dContainerKind,
    /// Optional manifest payloads rejected after their byte spans were admitted.
    pub(crate) manifest_diagnostics: Vec<manifest::MetadataDiagnostic>,
    /// Optional archive payload failures; complete stored bytes remain in `source_image`.
    pub(crate) entry_diagnostics: Vec<String>,
    /// Entry payload views, keyed by archive path.
    inflated_entries: BTreeMap<String, View<'a>>,
    /// Entry indices per native scope key, in entry order.
    scope_entry_indices: std::collections::HashMap<String, Vec<usize>>,
    /// Parsed `MetaStream` entries, memoized for the scan's duration.
    metastream_cache: std::cell::RefCell<
        std::collections::HashMap<String, std::rc::Rc<crate::metastream::MetaStream>>,
    >,
}

/// The framing outcome of one text B-rep member.
pub(crate) enum TextBrepFraming {
    Parsed(Box<cadmpeg_asm::sat::TextStream>),
    Unframed(cadmpeg_asm::stream_error::StreamError),
    Malformed(cadmpeg_asm::stream_error::StreamError),
    UnsupportedLength(cadmpeg_asm::stream_error::StreamError),
}

impl<'a> ContainerScan<'a> {
    /// Returns an entry payload retained during the single archive scan.
    pub(crate) fn entry_bytes(&self, name: &str) -> Result<&'a [u8], CodecError> {
        self.entry_view(name)
            .map(View::window)
            .ok_or_else(|| CodecError::malformed(format_args!("entry {name} not found")))
    }

    /// Returns an entry's payload view.
    pub(crate) fn entry_view(&self, name: &str) -> Option<View<'a>> {
        self.inflated_entries.get(name).copied()
    }

    /// The first entry, in entry order, whose native scope key is `scope` and
    /// which is a Design stream of `expected_role`. Equivalent to a linear
    /// `find` over `entries` with both predicates.
    pub(crate) fn design_stream_entry_for_scope(
        &self,
        expected_role: ContainerRole,
        scope: &str,
    ) -> Option<&ContainerEntry> {
        self.scope_entry_indices
            .get(scope)?
            .iter()
            .filter_map(|index| self.entries.get(*index))
            .find(|entry| self.is_design_stream(entry, expected_role))
    }

    /// The parse of one `MetaStream` entry, computed at most once per scan.
    pub(crate) fn parsed_metastream(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<std::rc::Rc<crate::metastream::MetaStream>, CodecError> {
        if let Some(cached) = self.metastream_cache.borrow().get(name) {
            return Ok(std::rc::Rc::clone(cached));
        }
        let parsed = crate::metastream::parse(ctx, self.entry_bytes(name)?, name)?;
        let mut cache = self.metastream_cache.borrow_mut();
        ctx.reserve_map(&mut cache, 1, "cache F3D MetaStream")?;
        let key = ctx.copy_retained_text(name, "cache F3D MetaStream name")?;
        let parsed = std::rc::Rc::new(parsed);
        cache.insert(key, std::rc::Rc::clone(&parsed));
        Ok(parsed)
    }

    /// Exact archive folder of the manifest-selected Design asset. An outer
    /// F3Z archive has no folder of its own; each member has one.
    pub(crate) fn design_asset_folder(&self) -> Option<&str> {
        match &self.kind {
            F3dContainerKind::Document {
                design_asset_folder,
                ..
            } => Some(design_asset_folder),
            F3dContainerKind::MultiDocument { .. } => None,
        }
    }

    /// Whether `name` is inside the manifest-selected Design asset folder.
    pub(crate) fn belongs_to_design_asset(&self, name: &str) -> bool {
        self.design_asset_folder().is_some_and(|folder| {
            name.strip_prefix(folder)
                .is_some_and(|suffix| suffix.starts_with('/'))
        })
    }

    /// Whether `entry` has `expected_role` inside the manifest-selected
    /// Design asset.
    pub(crate) fn is_design_asset_entry(
        &self,
        entry: &ContainerEntry,
        expected_role: ContainerRole,
    ) -> bool {
        entry.role == expected_role && self.belongs_to_design_asset(&entry.name)
    }

    /// Whether `entry` is a stream of `expected_role` in a Design segment of
    /// the manifest-selected Design asset.
    pub(crate) fn is_design_stream(
        &self,
        entry: &ContainerEntry,
        expected_role: ContainerRole,
    ) -> bool {
        if !self.is_design_asset_entry(entry, expected_role) {
            return false;
        }
        self.asset_segment(&entry.name).is_some_and(|segment| {
            segment == "Design1" || is_numbered_segment(segment, "FusionDesignSegmentType")
        })
    }

    /// Whether `entry` is a `BulkStream.dat` in an ACT segment of the
    /// manifest-selected Design asset.
    pub(crate) fn is_act_stream(&self, entry: &ContainerEntry) -> bool {
        self.is_design_asset_entry(entry, ContainerRole::Bulkstream)
            && self
                .asset_segment(&entry.name)
                .is_some_and(|segment| is_numbered_segment(segment, "FusionACTSegmentType"))
    }

    fn asset_segment<'n>(&self, name: &'n str) -> Option<&'n str> {
        let relative = self
            .design_asset_folder()
            .and_then(|folder| name.strip_prefix(folder))?
            .strip_prefix('/')?;
        relative.split_once('/').map(|(segment, _)| segment)
    }
}

fn is_numbered_segment(segment: &str, prefix: &str) -> bool {
    segment.strip_prefix(prefix).is_some_and(|ordinal| {
        !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Read and classify every entry, decoding ASM headers for BREP streams.
///
/// Readable entries are registered as stored slices or decompressed spaces.
/// Unreadable optional assets retain their declared storage and source bytes.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    let source_image = root.window();
    let archive = ArchiveSnapshot::new(ctx, root)?;

    let mut entries = Vec::new();
    let mut breps = Vec::new();
    let mut inflated_entries = BTreeMap::new();
    let mut entry_diagnostics = Vec::new();

    for file in archive.entries() {
        let name = ctx.copy_retained_text(&file.name, "retain F3D entry name")?;
        let role = classify(&name);
        let compression = file.compression;
        let compressed_size = file.compressed_size;
        let uncompressed_size = file.uncompressed_size;
        let mut attributes = BTreeMap::new();
        if let cadmpeg_container::ZipCompression::Unsupported(method) = compression {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("compression", "F3D compression declaration key")?,
                ctx.format_retained(format_args!("{method}"), "F3D compression declaration")?,
                "F3D compression declaration attribute",
            )?;
        }
        if file.encrypted {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("encrypted", "F3D encryption declaration key")?,
                ctx.copy_retained_text("true", "F3D encryption declaration")?,
                "F3D encryption declaration attribute",
            )?;
        }

        let is_brep = role == ContainerRole::BrepSmbh || role == ContainerRole::BrepSmb;
        let view = match archive.open(ctx, &file.name) {
            Ok(view) => Some(view),
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error)
                if matches!(
                    role,
                    ContainerRole::Preview
                        | ContainerRole::Image
                        | ContainerRole::OgsCache
                        | ContainerRole::Other
                ) =>
            {
                let diagnostic = ctx.format_retained(
                    format_args!("{name} cannot be opened: {error}; stored payload retained"),
                    "F3D optional entry diagnostic",
                )?;
                ctx.insert_btree_map(
                    &mut attributes,
                    ctx.copy_retained_text("payload_error", "F3D optional entry diagnostic key")?,
                    ctx.copy_retained_text(&diagnostic, "F3D optional entry summary diagnostic")?,
                    "F3D optional entry attributes",
                )?;
                ctx.push_vec(
                    &mut entry_diagnostics,
                    diagnostic,
                    "F3D optional entry diagnostics",
                )?;
                None
            }
            Err(error) => return Err(error),
        };
        let buf = view.map_or(&[][..], |view| view.window());
        if is_brep {
            let kernel = if asm_header::has_asm_magic(buf) {
                match asm_header::parse(ctx, buf)? {
                    Some(header) => Some(KernelFraming::Asm {
                        solved_record_limit: asm_header::solved_record_limit_with_header(
                            ctx, buf, &header,
                        )?,
                        header,
                    }),
                    None => None,
                }
            } else {
                acis_header::parse(ctx, buf)?.map(KernelFraming::Acis)
            };
            let solved_record_limit = kernel.as_ref().and_then(KernelFraming::solved_record_limit);
            let sha = cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                buf,
                "retain source digest",
            )?;

            ctx.insert_btree_map(
                &mut attributes,
                "asm_magic".to_owned(),
                asm_magic_label(buf),
                "index F3D container attributes",
            )
            .map(|_| ())?;
            if let Some(h) = kernel.as_ref().and_then(KernelFraming::asm_header) {
                ctx.insert_btree_map(
                    &mut attributes,
                    "asm_width".to_owned(),
                    h.width.to_string(),
                    "index F3D container attributes",
                )
                .map(|_| ())?;
                if let Some(v) = h.metadata.save_format_version {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "acis_save_format_version".to_owned(),
                        v.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(v) = asm_header::record_count(buf) {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "asm_record_count".to_owned(),
                        v.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(v) = h.metadata.entity_count {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "asm_entity_count".to_owned(),
                        v.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(v) = h.metadata.flags {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "asm_flags".to_owned(),
                        v.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(pf) = &h.metadata.product_family {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "product_family".to_owned(),
                        ctx.copy_retained_text(pf, "retain F3D container attribute")?,
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(pv) = &h.metadata.product_version {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "product_version".to_owned(),
                        ctx.copy_retained_text(pv, "retain F3D container attribute")?,
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(sd) = &h.metadata.save_date {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "save_date".to_owned(),
                        ctx.copy_retained_text(sd, "retain F3D container attribute")?,
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(s) = h.metadata.scale {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "scale".to_owned(),
                        format!("{s}"),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(r) = h.metadata.linear {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "resabs".to_owned(),
                        format!("{r}"),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                if let Some(r) = h.metadata.angular {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "resnor".to_owned(),
                        format!("{r}"),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
            }
            match solved_record_limit {
                Some(offset) => {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "history_partition_offset".to_owned(),
                        offset.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                    ctx.insert_btree_map(
                        &mut attributes,
                        "solved_record_len".to_owned(),
                        offset.to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
                None => {
                    ctx.insert_btree_map(
                        &mut attributes,
                        "history_partition_offset".to_owned(),
                        "none".to_string(),
                        "index F3D container attributes",
                    )
                    .map(|_| ())?;
                }
            }
            ctx.insert_btree_map(
                &mut attributes,
                "sha256".to_owned(),
                sha.as_str().to_owned(),
                "index F3D container attributes",
            )
            .map(|_| ())?;

            ctx.push_vec(
                &mut breps,
                BrepFacts {
                    name: ctx.copy_retained_text(&name, "retain F3D BREP name")?,
                    uncompressed_len: uncompressed_size,
                    kernel,
                    sha256: sha,
                },
                "collect F3D BREP facts",
            )?;
        }

        let declared_storage = if file.encrypted {
            Ok(EntryStorage::payload_only(
                VerbatimLabel::None,
                compressed_size,
            ))
        } else {
            compression.storage(compressed_size, uncompressed_size)
        };
        let storage = match declared_storage {
            Ok(storage) => storage,
            Err(message) => {
                ctx.insert_btree_map(
                    &mut attributes,
                    "storage_declaration".to_owned(),
                    format!("{message}: {compressed_size}/{uncompressed_size}"),
                    "index F3D container attributes",
                )
                .map(|_| ())?;
                EntryStorage::payload_only(VerbatimLabel::Stored, uncompressed_size)
            }
        };
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.copy_retained_text(&name, "retain F3D summary entry name")?,
                role,
                storage,
                attributes,
            },
            "collect F3D container entries",
        )?;
        if let Some(view) = view {
            ctx.insert_btree_map(
                &mut inflated_entries,
                name,
                view,
                "index F3D inflated entries",
            )?;
        }
    }

    // The parse strategy and the dialect row are chosen together, from the same
    // discriminants, before anything semantic is read. Classifying here is what
    // keeps the report from re-deriving an identity the parse already settled.
    let root_document_members = root_f3d_members(ctx, &inflated_entries)?;
    let mut manifest_diagnostics = Vec::new();
    let kind = if let Some(top_level_manifest) = inflated_entries.get("Manifest.dat") {
        let top_level_manifest = manifest::parse_top_level(ctx, top_level_manifest.window())?;
        let matched = F3dDialect::classify_document(ctx, top_level_manifest.declared_version())?;
        let design_asset_folder = manifest::resolve_design_folder(
            ctx,
            &top_level_manifest,
            inflated_entries.keys().map(String::as_str),
            |name| inflated_entries.get(name).map(|view| view.window()),
        )?;
        manifest_diagnostics = top_level_manifest.diagnostics;
        F3dContainerKind::Document {
            design_asset_folder,
            matched,
        }
    } else if inflated_entries.contains_key("Manifest.json")
        && inflated_entries.contains_key("DesignDescription.json")
        && !root_document_members.is_empty()
    {
        F3dContainerKind::MultiDocument {
            matched: F3dDialect::classify_f3z(ctx, &root_document_members)?,
        }
    } else {
        return Err(CodecError::Malformed(
            "Fusion ZIP has neither a top-level Manifest.dat nor the F3Z manifest set".into(),
        ));
    };

    let mut scope_entry_indices = std::collections::HashMap::<String, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        let scope = crate::ids::native_scope_charged(ctx, &entry.name)?;
        ctx.push_hash_group(
            &mut scope_entry_indices,
            scope,
            index,
            "index F3D native scopes",
            "index F3D scope entries",
        )?;
    }

    let mut scan = ContainerScan {
        source_image,
        entries,
        breps,
        text_breps: std::collections::HashMap::new(),
        kind,
        manifest_diagnostics,
        entry_diagnostics,
        inflated_entries,
        scope_entry_indices,
        metastream_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
    };
    for index in 0..scan.entries.len() {
        let entry = &scan.entries[index];
        if entry.role != ContainerRole::BrepText || !scan.belongs_to_design_asset(&entry.name) {
            continue;
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let framing = match cadmpeg_asm::sat::parse(ctx, bytes) {
            Ok(stream) => TextBrepFraming::Parsed(Box::new(stream)),
            Err(cadmpeg_asm::stream_error::StreamFailure::Parse(error)) => {
                TextBrepFraming::Unframed(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Malformed(error)) => {
                TextBrepFraming::Malformed(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::NotImplemented(error)) => {
                TextBrepFraming::UnsupportedLength(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                return Err(error.into())
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                return Err(error.into_codec_error())
            }
        };
        ctx.reserve_map(&mut scan.text_breps, 1, "retain F3D text B-rep framing")?;
        let name = ctx.copy_retained_text(&entry.name, "retain F3D text B-rep name")?;
        scan.text_breps.insert(name, framing);
    }
    Ok(scan)
}

/// Build a [`ContainerSummary`] without assigning model authority from a ZIP
/// extension. Design body bindings perform the model selection during decode.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    dialects: cadmpeg_core::dialect::DialectLayers,
) -> Result<ContainerSummary, CodecError> {
    Ok(ContainerSummary::classified(
        dialects,
        cadmpeg_ir::ContainerKind::Zip,
        copy_summary_entries(ctx, &scan.entries)?,
        Vec::new(),
        summary_notes(ctx, scan, SummaryScope::ContainerOnly)?,
    ))
}

/// Whether the caller transferred beyond container metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SummaryScope {
    /// Inspection or a container-only decode.
    ContainerOnly,
    /// A decode that attempted the document model.
    FullDecode,
}

/// Container notes shared by inspection and decode report construction.
pub(crate) fn summary_notes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    scope: SummaryScope,
) -> Result<Vec<String>, CodecError> {
    let mut notes = Vec::new();
    if let Some(folder) = scan.design_asset_folder() {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("Design asset folder (from manifests): {folder}"),
            "collect F3D summary notes",
            "retain F3D summary note",
        )?;
    } else {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("outer F3Z archive; each F3D member selects its own Design asset"),
            "collect F3D summary notes",
            "retain F3D summary note",
        )?;
    }
    let design_brep_count = design_breps(scan).count();
    ctx.push_formatted_retained(
        &mut notes,
        format_args!(
        "{design_brep_count} ASM BREP stream(s); Design body-to-blob bindings select model geometry"
    ),
        "collect F3D summary notes",
        "retain F3D summary note",
    )?;
    if design_brep_count != scan.breps.len() {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!(
                "{} ASM BREP stream(s) belong to non-Design assets",
                scan.breps.len() - design_brep_count
            ),
            "collect F3D summary notes",
            "retain F3D summary note",
        )?;
    }
    let history_brep_count = history_breps(scan).count();
    match history_brep_count {
        0 => {
            if design_brep_count != 0 {
                ctx.push_formatted_retained(
                    &mut notes,
                    format_args!("no BREP header declares a history partition"),
                    "collect F3D summary notes",
                    "retain F3D summary note",
                )?;
            }
        }
        1 => {
            if let Some(history) = history_breps(scan).next() {
                ctx.push_formatted_retained(
                    &mut notes,
                    format_args!(
                        "history-bearing BREP: {} ({} bytes uncompressed)",
                        history.name, history.uncompressed_len
                    ),
                    "collect F3D summary notes",
                    "retain F3D summary note",
                )?;
            }
        }
        count => ctx.push_formatted_retained(
            &mut notes,
            format_args!(
                "{count} history-bearing BREPs; each history graph is decoded independently"
            ),
            "collect F3D summary notes",
            "retain F3D summary note",
        )?,
    }
    if scope == SummaryScope::ContainerOnly {
        ctx.push_formatted_retained(&mut notes, format_args!(
            "container-level inspection only; run `decode` to resolve Design body bindings and build \
             each referenced BREP graph"
        ), "collect F3D summary notes", "retain F3D summary note")?;
    }

    Ok(notes)
}

/// Root-level `*.f3d` member names, sorted by archive path.
///
/// The third clause of the F3Z discriminant: a member whose name carries no `/`
/// and whose extension is `f3d`, case-insensitively. Each name is returned as
/// the archive spells it, and the order is the entry map's, which is sorted
/// rather than the archive's own sequence.
fn root_f3d_members<'a>(
    ctx: &DecodeContext<'_>,
    entries: &'a BTreeMap<String, View<'_>>,
) -> Result<Vec<&'a str>, CodecError> {
    let mut members = Vec::new();
    for name in entries.keys().map(String::as_str) {
        if !name.contains('/') && is_f3d_name(name) {
            ctx.push_vec(&mut members, name, "collect F3Z document members")?;
        }
    }
    Ok(members)
}

/// Whether an archive path names an F3D document by extension.
pub(crate) fn is_f3d_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("f3d"))
}

/// Iterate over every BREP whose parsed header sets the history-partition bit.
/// The extension is not used as a semantic substitute for the header flag.
pub(crate) fn history_breps<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s BrepFacts> + 's {
    design_breps(scan).filter(|brep| {
        brep.kernel
            .as_ref()
            .and_then(KernelFraming::asm_header)
            .is_some_and(|header| header.metadata.has_history_partition())
    })
}

/// Return the history-bearing BREP only when the header relation is unique.
pub(crate) fn select_history_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    let mut candidates = history_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Return one unambiguous BREP for compatibility metadata and reporting.
pub(crate) fn select_fallback_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    if let Some(history) = select_history_brep(scan) {
        return Some(history);
    }
    let mut candidates = design_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Iterate over binary ASM BREP entries inside the manifest-selected Design
/// asset.
pub(crate) fn design_breps<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s BrepFacts> + 's {
    scan.breps
        .iter()
        .filter(|brep| scan.belongs_to_design_asset(&brep.name))
}

/// Names of the text-encoded ASM BREP entries, in archive order.
///
/// [`ContainerScan::breps`] holds binary BREP candidate facts. Text streams are
/// admitted separately into [`ContainerScan::text_breps`]. Carrier presence is
/// independent of geometry transfer: a text-only document has a carrier even
/// when its transfer fails.
pub(crate) fn text_brep_names<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s str> + 's {
    scan.entries
        .iter()
        .filter(|entry| {
            entry.role == ContainerRole::BrepText && scan.belongs_to_design_asset(&entry.name)
        })
        .map(|entry| entry.name.as_str())
}

fn asm_magic_label(bytes: &[u8]) -> String {
    if asm_header::has_asm_magic(bytes) {
        // Both magics are the 15-byte prefix plus the width digit; byte 15 is
        // save-format-version data.
        String::from_utf8_lossy(&bytes[..15]).to_string()
    } else {
        "absent".to_string()
    }
}

#[cfg(test)]
mod tests {

    use super::is_f3d_name;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
    use std::collections::BTreeMap;
    use std::io::Write;

    fn scan_refusal_at(operation: &str) -> cadmpeg_core::CodecError {
        let bytes = crate::test_support::zip_test::f3d_with_smbh(&[]);
        for limit in 0..256 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let arena = DecodeArena::new();
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            if let Err(error) = super::scan(&ctx, root) {
                if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                    if refusal.operation == operation)
                {
                    return error;
                }
            }
        }
        panic!("operation {operation} did not refuse a collection limit");
    }

    #[test]
    fn container_attributes_refuse_collection_limit() {
        let error = scan_refusal_at("index F3D container attributes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_brep_facts_refuse_collection_limit() {
        let error = scan_refusal_at("collect F3D BREP facts");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_entries_refuse_collection_limit() {
        let error = scan_refusal_at("collect F3D container entries");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_inflated_index_refuses_collection_limit() {
        let error = scan_refusal_at("index F3D inflated entries");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_native_scope_index_refuses_collection_limit() {
        let error = scan_refusal_at("index F3D native scopes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_scope_entry_index_refuses_collection_limit() {
        let error = scan_refusal_at("index F3D scope entries");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn container_entry_name_refuses_retained_limit() {
        let bytes = crate::test_support::zip_test::f3d_with_smbh(&[]);
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain F3D entry name",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                super::scan(&ctx, root).map(|_| ())
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D entry name")
        );
    }

    #[test]
    fn container_f3z_member_list_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let entries = BTreeMap::from([("document.f3d".to_owned(), View::over_retained(&[]))]);
        let error = super::root_f3d_members(&ctx, &entries).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3Z document members")
        );
    }

    #[test]
    fn container_text_brep_framing_refuses_collection_limit() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.sat", stored)
            .unwrap();
        zip.write_all(b"bad text BREP").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        for limit in 0..256 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let arena = DecodeArena::new();
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            if let Err(error) = super::scan(&ctx, root) {
                if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                    if refusal.operation == "retain F3D text B-rep framing")
                {
                    return;
                }
            }
        }
        panic!("text BREP framing did not refuse a collection limit");
    }

    #[test]
    fn container_metastream_cache_refuses_collection_limit() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        let name = "FusionAssetName[Active]/Design1/MetaStream.dat";
        zip.start_file(name, stored).unwrap();
        zip.write_all(&crate::test_support::streams_test::design_metastream(&[]))
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = super::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let Err(error) = scan.parsed_metastream(&limited, name) else {
            panic!("MetaStream cache entry must refuse");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "cache F3D MetaStream")
        );
    }

    #[test]
    fn container_metastream_cache_name_refuses_retained_limit() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        let name = "FusionAssetName[Active]/Design1/MetaStream.dat";
        zip.start_file(name, stored).unwrap();
        zip.write_all(&crate::test_support::streams_test::design_metastream(&[]))
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        super::scan(&ctx, root).unwrap();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "cache F3D MetaStream name",
            |cap| {
                let mut limited_policy = DecodePolicy::service();
                limited_policy.limits.max_retained_bytes = cap;
                let (limited, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
                let (unlimited, root) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let fresh_scan = super::scan(&unlimited, root).unwrap();
                fresh_scan.parsed_metastream(&limited, name).map(|_| ())
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "cache F3D MetaStream name")
        );
    }

    #[test]
    fn f3d_name_requires_a_nonempty_stem_and_case_insensitive_extension() {
        assert!(is_f3d_name("part.f3d"));
        assert!(is_f3d_name("folder/part.F3D"));
        assert!(!is_f3d_name(".f3d"));
        assert!(!is_f3d_name("part.f3d.tmp"));
    }
}
