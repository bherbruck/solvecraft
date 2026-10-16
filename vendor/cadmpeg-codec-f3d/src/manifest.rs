// SPDX-License-Identifier: Apache-2.0
//! Parse the document and asset manifests that assign archive folders to
//! Fusion assets.

use cadmpeg_core::decode::u64_from_index;

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use crate::bytes::is_guid_hyphenated;
use crate::bytes::utf16::Utf16View;

const MAX_MANIFEST_STRING_UNITS: usize = 4 * 1024;
const MAX_REGISTRY_ENTRIES: usize = 64;
const MAX_ASSET_FOLDERS: usize = 64;
const MAX_TOP_LEVEL_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const DESIGN_ASSET_TYPE: &str = "FusionAssetType";
/// The top-level manifest version whose layout this codec declares. Every
/// other readable version is parsed with the same layout and classified on the
/// recovery row.
pub(crate) const TOP_LEVEL_MANIFEST_VERSION: &str = "3-2-0-0";

fn parse_malformed(
    ctx: &DecodeContext<'_>,
    field: &str,
    message: impl std::fmt::Display,
) -> CodecError {
    match ctx.format_retained(
        format_args!("F3D {field}: {message}"),
        "describe malformed F3D manifest",
    ) {
        Ok(text) => CodecError::Malformed(text),
        Err(refusal) => refusal,
    }
}

/// A parse error holds temporary diagnostic storage until it escapes its probe.
#[derive(Debug)]
struct ManifestFailure<'ctx> {
    error: CodecError,
    reservation: Option<ScopedReservation<'ctx>>,
}
impl ManifestFailure<'_> {
    fn is_resource(&self) -> bool {
        matches!(self.error, CodecError::ResourceLimit(_))
    }
    fn into_codec(self) -> CodecError {
        if let Some(reservation) = self.reservation {
            if let Err(refusal) = reservation.commit() {
                return refusal;
            }
        }
        self.error
    }
}
impl From<CodecError> for ManifestFailure<'_> {
    fn from(error: CodecError) -> Self {
        Self {
            error,
            reservation: None,
        }
    }
}
impl std::fmt::Display for ManifestFailure<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, formatter)
    }
}
fn probe_malformed<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    field: &str,
    message: impl std::fmt::Display,
) -> ManifestFailure<'ctx> {
    match ctx.format_scoped(
        format_args!("F3D {field}: {message}"),
        "describe malformed F3D manifest",
    ) {
        Ok((text, reservation)) => ManifestFailure {
            error: CodecError::Malformed(text),
            reservation: Some(reservation),
        },
        Err(refusal) => refusal.into(),
    }
}

const GENERATED_DESIGN_ASSET_BASE: &str = "FusionAssetName";
pub(crate) const GENERATED_DESIGN_ASSET_FOLDER: &str = "FusionAssetName[Active]";

const GENERATED_DOCUMENT_GUID: &str = "00000000-0000-4000-8000-000000000001";
const GENERATED_DOCUMENT_ASSET_GUID: &str = "00000000-0000-4000-8000-000000000002";
const GENERATED_ASSET_FOLDER_GUID: &str = "00000000-0000-4000-8000-000000000003";
const GENERATED_ASSET_GUID: &str = "00000000-0000-4000-8000-000000000004";
const GENERATED_PHYSICAL_CHANGE_GUID: &str = "00000000-0000-4000-8000-000000000005";

/// Fields from the top-level manifest that govern asset-folder ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopLevelManifest {
    /// Leading length-prefixed ASCII version field, as the cursor read it.
    ///
    /// [`parse_top_level`] reads every manifest with the `3-2-0-0` layout and
    /// keeps whatever version the field declared, so this is the reading rather
    /// than a constant. It is the evidence the dialect match records, kept
    /// beside the parse instead of re-derived at the report boundary.
    version: Option<String>,
    asset_folder_bases: Vec<String>,
    pub(crate) diagnostics: Vec<MetadataDiagnostic>,
}

impl TopLevelManifest {
    /// The version field the top-level manifest declared, verbatim.
    pub(crate) fn declared_version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetadataDiagnostic {
    pub(crate) field: &'static str,
    pub(crate) offset: usize,
}

#[derive(Clone, Copy)]
enum MetadataString {
    Text,
    Guid,
}

/// Prefix fields that identify one asset manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetManifestHeader<'a> {
    base_name: Utf16View<'a>,
    kind: AssetKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssetKind<'a> {
    Design { fusion_subtype: Option<&'a str> },
    Other,
}

struct Cursor<'a, 'ctx, 'arena> {
    view: View<'a>,
    ctx: &'ctx DecodeContext<'arena>,
}

impl<'a, 'ctx, 'arena> Cursor<'a, 'ctx, 'arena> {
    fn new(ctx: &'ctx DecodeContext<'arena>, bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
            ctx,
        }
    }

    fn from_offset(
        ctx: &'ctx DecodeContext<'arena>,
        bytes: &'a [u8],
        at: usize,
    ) -> Result<Self, ManifestFailure<'ctx>> {
        let mut view = View::over_retained(bytes);
        view.seek(at)
            .ok_or_else(|| truncated(ctx, "manifest offset"))?;
        Ok(Self { view, ctx })
    }

    fn position(&self) -> usize {
        self.view.position()
    }

    fn exhausted(&self) -> bool {
        self.view.is_empty()
    }

    fn u8(&mut self, field: &str) -> Result<u8, ManifestFailure<'ctx>> {
        self.view.u8().ok_or_else(|| truncated(self.ctx, field))
    }

    fn expect_u8(&mut self, field: &str, expected: u8) -> Result<(), ManifestFailure<'ctx>> {
        let actual = self.u8(field)?;
        if actual != expected {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("expected {expected}, found {actual}"),
            ));
        }
        Ok(())
    }

    fn u32(&mut self, field: &str) -> Result<u32, ManifestFailure<'ctx>> {
        self.view.u32_le().ok_or_else(|| truncated(self.ctx, field))
    }

    fn expect_u32(&mut self, field: &str, expected: u32) -> Result<(), ManifestFailure<'ctx>> {
        let actual = self.u32(field)?;
        if actual != expected {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("expected {expected}, found {actual}"),
            ));
        }
        Ok(())
    }

    fn ascii(&mut self, field: &str) -> Result<&'a str, ManifestFailure<'ctx>> {
        let raw = self.ascii_bytes(field)?;
        if !raw.iter().all(|byte| matches!(byte, 0x20..=0x7e)) {
            return Err(probe_malformed(
                self.ctx,
                field,
                "contains a non-printable ASCII byte",
            ));
        }
        std::str::from_utf8(raw)
            .map_err(|_| probe_malformed(self.ctx, field, "contains a non-printable ASCII byte"))
    }

    fn ascii_bytes(&mut self, field: &str) -> Result<&'a [u8], ManifestFailure<'ctx>> {
        let count = self.count(field, MAX_MANIFEST_STRING_UNITS)?;
        let raw = self
            .view
            .take(count)
            .ok_or_else(|| truncated(self.ctx, field))?;
        self.ctx
            .charge_work(u64_from_index(raw.len()) * 4, "decode F3D manifest ASCII")?;
        Ok(raw)
    }

    fn expect_ascii(&mut self, field: &str, expected: &str) -> Result<(), ManifestFailure<'ctx>> {
        let actual = self.ascii(field)?;
        if actual != expected {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("expected {expected:?}, found {actual:?}"),
            ));
        }
        Ok(())
    }

    fn utf16(&mut self, field: &str) -> Result<Utf16View<'a>, ManifestFailure<'ctx>> {
        let count = self.count(field, MAX_MANIFEST_STRING_UNITS)?;
        self.utf16_with_count(count, field)
    }

    fn utf16_with_count(
        &mut self,
        count: usize,
        field: &str,
    ) -> Result<Utf16View<'a>, ManifestFailure<'ctx>> {
        let raw = self.utf16_bytes_with_count(count, field)?;
        Utf16View::new(raw)
            .ok_or_else(|| probe_malformed(self.ctx, field, "contains invalid UTF-16LE"))
    }

    fn utf16_bytes_with_count(
        &mut self,
        count: usize,
        field: &str,
    ) -> Result<&'a [u8], ManifestFailure<'ctx>> {
        let needed = count
            .checked_mul(2)
            .ok_or_else(|| truncated(self.ctx, field))?;
        let raw = self
            .view
            .take(needed)
            .ok_or_else(|| truncated(self.ctx, field))?;
        self.ctx
            .charge_work(u64_from_index(needed) * 8, "decode F3D manifest UTF-16")?;
        Ok(raw)
    }

    fn metadata_utf16(
        &mut self,
        field: &'static str,
        kind: MetadataString,
        diagnostics: &mut Vec<MetadataDiagnostic>,
    ) -> Result<(), ManifestFailure<'ctx>> {
        let offset = self.position();
        let count = self.count(field, MAX_MANIFEST_STRING_UNITS)?;
        let raw = self.utf16_bytes_with_count(count, field)?;
        let valid = Utf16View::new(raw).is_some_and(|value| match kind {
            MetadataString::Text => true,
            MetadataString::Guid => value.is_guid_hyphenated(),
        });
        if !valid {
            self.ctx.push_vec(
                diagnostics,
                MetadataDiagnostic { field, offset },
                "F3D manifest metadata diagnostics",
            )?;
        }
        Ok(())
    }

    fn expect_utf16(&mut self, field: &str, expected: &str) -> Result<(), ManifestFailure<'ctx>> {
        let actual = self.utf16(field)?;
        if !actual.eq_str(expected) {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("expected {expected:?}, found {actual:?}"),
            ));
        }
        Ok(())
    }

    fn guid(&mut self, field: &str) -> Result<Utf16View<'a>, ManifestFailure<'ctx>> {
        let value = self.utf16(field)?;
        if !value.is_guid_hyphenated() {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("invalid GUID {value:?}"),
            ));
        }
        Ok(value)
    }

    fn count(&mut self, field: &str, max: usize) -> Result<usize, ManifestFailure<'ctx>> {
        let count = usize::try_from(self.u32(field)?)
            .map_err(|_| probe_malformed(self.ctx, field, "count does not fit memory"))?;
        if count > max {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("count {count} exceeds the limit {max}"),
            ));
        }
        Ok(count)
    }

    fn finish(self, field: &str) -> Result<(), ManifestFailure<'ctx>> {
        if !self.view.is_empty() {
            return Err(probe_malformed(
                self.ctx,
                field,
                format_args!("{} trailing byte(s)", self.view.remaining()),
            ));
        }
        Ok(())
    }
}

/// Parse the top-level `Manifest.dat` header, capability registry, and exact
/// asset-folder tail.
///
/// The version field selects no layout. Every version is parsed with
/// the `3-2-0-0` layout. `FusionDocType`, `.f3d`, the registry grammar and the
/// exact asset-folder tail establish the layout. Descriptive string payloads
/// do not control folder ownership. A
/// failed attempt remains a structural error and names the declared version
/// alongside the failed layout diagnosis.
pub(crate) fn parse_top_level(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<TopLevelManifest, CodecError> {
    let parsed = (|| {
        if bytes.len() > MAX_TOP_LEVEL_MANIFEST_BYTES {
            return Err(probe_malformed(
                ctx,
                "top-level manifest",
                format_args!(
                    "{} bytes exceed the limit {MAX_TOP_LEVEL_MANIFEST_BYTES}",
                    bytes.len()
                ),
            ));
        }
        let mut cursor = Cursor::new(ctx, bytes);
        let raw_version = cursor.ascii_bytes("top-level manifest version")?;
        let version = std::str::from_utf8(raw_version)
            .ok()
            .filter(|value| value.bytes().all(|byte| matches!(byte, 0x20..=0x7e)));
        let mut diagnostics = Vec::new();
        let mut asset_storage = ctx.reserve_scoped(0, "F3D manifest asset views")?;
        let parsed_body = asset_storage
            .with_storage(|| parse_top_level_body(ctx, bytes, cursor, &mut diagnostics));
        let asset_folder_bases = parsed_body.map_err(|error| {
            if error.is_resource() {
                error
            } else {
                probe_malformed(ctx, "top-level manifest", format_args!(
                    "the {TOP_LEVEL_MANIFEST_VERSION} grammar does not fit; declared version {}: {error}",
                    version.unwrap_or("unreadable")
                ))
            }
        })?;
        let version = version
            .map(|version| ctx.copy_retained_text(version, "retain F3D manifest ASCII"))
            .transpose()?;
        let mut diagnostics = ctx.try_collect_vec(
            diagnostics.into_iter().map(Ok::<_, CodecError>),
            "retain F3D manifest metadata diagnostics",
        )?;
        if version.is_none() {
            ctx.push_vec(
                &mut diagnostics,
                MetadataDiagnostic {
                    field: "top-level manifest version",
                    offset: 0,
                },
                "F3D manifest metadata diagnostics",
            )?;
        }
        let asset_folder_bases = ctx.try_collect_vec(
            asset_folder_bases
                .into_iter()
                .map(|base| base.to_retained(ctx, "retain F3D manifest UTF-16")),
            "collect F3D retained asset folders",
        )?;
        Ok(TopLevelManifest {
            version,
            asset_folder_bases,
            diagnostics,
        })
    })();
    parsed.map_err(ManifestFailure::into_codec)
}

/// The `3-2-0-0` top-level manifest layout after the version field.
fn parse_top_level_body<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    mut cursor: Cursor<'a, 'ctx, '_>,
    diagnostics: &mut Vec<MetadataDiagnostic>,
) -> Result<Vec<Utf16View<'a>>, ManifestFailure<'ctx>> {
    cursor.expect_ascii("top-level manifest kind", "FusionDocType")?;
    cursor.expect_utf16("top-level manifest extension", ".f3d")?;
    cursor.metadata_utf16(
        "top-level manifest display name",
        MetadataString::Text,
        diagnostics,
    )?;
    cursor.metadata_utf16(
        "top-level manifest description",
        MetadataString::Text,
        diagnostics,
    )?;
    cursor.metadata_utf16(
        "top-level manifest document GUID",
        MetadataString::Guid,
        diagnostics,
    )?;
    cursor.metadata_utf16(
        "top-level manifest document-asset GUID",
        MetadataString::Guid,
        diagnostics,
    )?;

    let generation = cursor.u32("top-level manifest generation")?;
    let registry_count = if generation == 1234 {
        let _generation_major = cursor.u32("top-level manifest generation major")?;
        let _generation_minor = cursor.u32("top-level manifest generation minor")?;
        let _generation_flags = cursor.u32("top-level manifest generation flags")?;
        bounded_count(
            ctx,
            cursor.u32("top-level manifest registry count")?,
            MAX_REGISTRY_ENTRIES,
            "top-level manifest registry count",
        )?
    } else {
        let _legacy_generation = cursor.u32("top-level manifest legacy generation")?;
        bounded_count(
            ctx,
            cursor.u32("top-level manifest registry count")?,
            MAX_REGISTRY_ENTRIES,
            "top-level manifest registry count",
        )?
    };

    let mut registry_storage = ctx.reserve_scoped(0, "F3D manifest registry storage")?;
    let mut registry_names = BTreeSet::new();
    for ordinal in 0..registry_count {
        let (field, _field_budget) = ctx.format_scoped(
            format_args!("top-level manifest registry name {ordinal}"),
            "describe F3D manifest field",
        )?;
        let name = cursor.ascii(&field)?;
        ctx.charge_work(
            u64_from_index(name.len()) * u64_from_index(registry_names.len() + 1) * 2,
            "compare F3D manifest registry names",
        )?;
        if name.is_empty() || registry_names.contains(&name) {
            return Err(probe_malformed(
                ctx,
                "top-level manifest registry",
                format_args!("empty or duplicate name {name:?}"),
            ));
        }
        registry_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut registry_names,
                name,
                "index F3D manifest registry names",
            )
        })?;
        let (field, _field_budget) = ctx.format_scoped(
            format_args!("top-level manifest registry value {ordinal}"),
            "describe F3D manifest field",
        )?;
        let _value = cursor.u32(&field)?;
    }

    parse_asset_tail(ctx, bytes, cursor.position())
}

/// The asset-folder base run of the one exact tail framing.
fn parse_asset_tail<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
) -> Result<Vec<Utf16View<'a>>, ManifestFailure<'ctx>> {
    let mut selected = None;
    for at in bytes
        .len()
        .checked_sub(3)
        .into_iter()
        .flat_map(|last| start..last)
    {
        ctx.charge_work(1, "search F3D manifest asset tail")?;
        if bytes.get(at..at + 4) != Some(36_u32.to_le_bytes().as_slice()) {
            continue;
        }
        let candidate = match parse_asset_tail_at(ctx, bytes, at) {
            Ok(candidate) => candidate,
            Err(error) if error.is_resource() => return Err(error),
            Err(_) => continue,
        };
        if selected.replace(candidate).is_some() {
            return Err(probe_malformed(
                ctx,
                "top-level manifest asset-folder tail",
                "more than one exact tail framing is valid",
            ));
        }
    }
    selected.ok_or_else(|| {
        probe_malformed(
            ctx,
            "top-level manifest asset-folder tail",
            "no exact tail framing is valid",
        )
    })
}

fn parse_asset_tail_at<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    at: usize,
) -> Result<Vec<Utf16View<'a>>, ManifestFailure<'ctx>> {
    let mut cursor = Cursor::from_offset(ctx, bytes, at)?;
    let _active_asset_guid = cursor.guid("top-level manifest active-asset GUID")?;
    let asset_folder_count = bounded_nonzero_count(
        ctx,
        cursor.u32("top-level manifest asset-folder count")?,
        MAX_ASSET_FOLDERS,
        "top-level manifest asset-folder count",
    )?;
    let mut asset_folder_bases = Vec::new();
    for ordinal in 0..asset_folder_count {
        let (field, _field_budget) = ctx.format_scoped(
            format_args!("top-level manifest asset-folder base {ordinal}"),
            "describe F3D manifest field",
        )?;
        let base = cursor.utf16(&field)?;
        validate_asset_base_view(ctx, base)?;
        ctx.charge_work(
            u64_from_index(base.len()) * 2 * u64_from_index(asset_folder_bases.len()),
            "compare F3D manifest asset bases",
        )?;
        if asset_folder_bases.contains(&base) {
            return Err(probe_malformed(
                ctx,
                "top-level manifest asset-folder run",
                format_args!("duplicate base name {base:?}"),
            ));
        }
        ctx.push_vec(&mut asset_folder_bases, base, "collect F3D asset folders")?;
    }
    cursor.expect_u32("top-level manifest terminal word", 0)?;
    if cursor.exhausted() {
        return Ok(asset_folder_bases);
    }
    match cursor.u8("top-level manifest terminal byte")? {
        0 => {
            let display_name = cursor.utf16("top-level manifest terminal display name")?;
            if display_name.is_empty() {
                return Err(probe_malformed(
                    ctx,
                    "top-level manifest terminal display name",
                    "value is empty",
                ));
            }
            if !cursor.exhausted() {
                let lineage_urn = cursor.utf16("top-level manifest lineage URN")?;
                if lineage_urn.len() <= 4
                    || !lineage_urn
                        .chars()
                        .take(4)
                        .map(|ch| ch.to_ascii_lowercase())
                        .eq("urn:".chars())
                    || !lineage_urn.chars().skip(4).all(|ch| ch.is_ascii_graphic())
                {
                    return Err(probe_malformed(
                        ctx,
                        "top-level manifest lineage URN",
                        "value is not a nonempty ASCII URN",
                    ));
                }
            }
        }
        1 if !cursor.exhausted() => {
            cursor.expect_utf16("top-level manifest export marker", "NA_EXPORT")?;
        }
        1 => {}
        value => {
            return Err(probe_malformed(
                ctx,
                "top-level manifest terminal byte",
                format_args!("expected 0 or 1, found {value}"),
            ))
        }
    }
    cursor.finish("top-level manifest")?;

    Ok(asset_folder_bases)
}

/// Resolve the unique Design archive folder through the top-level folder run
/// and each listed folder's asset-manifest header.
pub(crate) fn resolve_design_folder<'a, 'n>(
    ctx: &DecodeContext<'_>,
    manifest: &TopLevelManifest,
    entry_names: impl IntoIterator<Item = &'n str>,
    mut entry_bytes: impl FnMut(&str) -> Option<&'a [u8]>,
) -> Result<String, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "F3D manifest folder lookups")?;
    let mut names = Vec::new();
    for name in entry_names {
        storage
            .with_storage(|| ctx.push_vec(&mut names, name, "index F3D manifest entry names"))?;
    }
    let mut design_folders = Vec::new();

    for base in &manifest.asset_folder_bases {
        let (active, _active_budget) =
            ctx.format_scoped(format_args!("{base}[Active]"), "name F3D active asset")?;
        let mut folder_matches = [None; 2];
        for (slot, candidate) in folder_matches
            .iter_mut()
            .zip([base.as_str(), active.as_str()])
        {
            for name in &names {
                let work = u64_from_index(candidate.len())
                    .checked_add(u64_from_index(name.len()))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("match F3D asset folders", u64::MAX - 1, u64::MAX)
                    })?;
                ctx.charge_work(work, "match F3D asset folders")?;
                if name
                    .strip_prefix(candidate)
                    .is_some_and(|suffix| suffix.starts_with('/'))
                {
                    *slot = Some(candidate);
                    break;
                }
            }
        }
        let folder = match folder_matches {
            [Some(folder), None] | [None, Some(folder)] => folder,
            [None, None] => {
                return Err(parse_malformed(
                    ctx,
                    "top-level manifest asset-folder run",
                    format_args!("listed base {base:?} has no archive folder"),
                ))
            }
            [Some(_), Some(_)] => {
                return Err(parse_malformed(
                    ctx,
                    "top-level manifest asset-folder run",
                    format_args!("listed base {base:?} resolves to multiple archive folders"),
                ))
            }
        };
        let (manifest_name, _manifest_name_budget) = ctx.format_scoped(
            format_args!("{folder}/Manifest.dat"),
            "name F3D asset manifest",
        )?;
        let bytes = entry_bytes(&manifest_name).ok_or_else(|| {
            parse_malformed(
                ctx,
                "asset manifest",
                format_args!("listed folder {folder:?} has no Manifest.dat"),
            )
        })?;
        let header = parse_asset_header(ctx, bytes)?;
        ctx.charge_work(
            u64_from_index(header.base_name.len()) * 2,
            "compare F3D asset manifest base",
        )?;
        if !header.base_name.eq_str(base) {
            return Err(parse_malformed(
                ctx,
                "asset manifest base name",
                format_args!(
                    "folder {folder:?} declares {:?}, expected {base:?}",
                    header.base_name
                ),
            ));
        }
        if matches!(
            header.kind,
            AssetKind::Design {
                fusion_subtype: None
            }
        ) {
            let name =
                ctx.format_scoped(format_args!("{folder}"), "retain F3D Design asset folder")?;
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut design_folders,
                    name,
                    "collect F3D Design asset folders",
                )
            })?;
        }
    }

    match design_folders.len() {
        1 => {
            let (name, reservation) = design_folders
                .pop()
                .ok_or_else(|| CodecError::malformed("missing admitted Design asset folder"))?;
            reservation.commit()?;
            Ok(name)
        }
        0 => Err(parse_malformed(
            ctx,
            "top-level manifest asset-folder run",
            "no listed folder declares the Design asset",
        )),
        _ => Err(parse_malformed(
            ctx,
            "top-level manifest asset-folder run",
            "more than one listed folder declares the Design asset",
        )),
    }
}

fn parse_asset_header<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<AssetManifestHeader<'a>, CodecError> {
    let parsed = (|| {
        let mut cursor = Cursor::new(ctx, bytes);
        let base_name = cursor.utf16("asset manifest base name")?;
        validate_asset_base_view(ctx, base_name)?;
        let _primary_guid = cursor.guid("asset manifest primary GUID")?;
        let _secondary_guid = cursor.guid("asset manifest secondary GUID")?;
        let asset_type = cursor.ascii("asset manifest asset type")?;
        if !asset_type.ends_with("AssetType") {
            return Err(probe_malformed(
                ctx,
                "asset manifest asset type",
                format_args!("invalid asset type {asset_type:?}"),
            ));
        }
        let kind = if asset_type == DESIGN_ASSET_TYPE {
            let revision = cursor.u32("Fusion asset manifest revision")?;
            let fusion_subtype = match revision {
            0 => {
                parse_revision_zero_design_asset(&mut cursor)?;
                cursor.finish("revision-0 Fusion asset manifest")?;
                None
            }
            10 => {
                parse_revision_ten_design_asset(&mut cursor)?;
                cursor.finish("revision-10 Fusion asset manifest")?;
                None
            }
            _ => parse_current_design_asset(&mut cursor).map_err(|error| {
                if error.is_resource() {
                    error
                } else {
                    probe_malformed(ctx,
                        "Fusion asset manifest",
                        format_args!(
                            "current grammar does not fit; declared revision {revision} is the probable cause: {error}"
                        ),
                    )
                }
            })?,
        };
            AssetKind::Design { fusion_subtype }
        } else {
            AssetKind::Other
        };
        Ok(AssetManifestHeader { base_name, kind })
    })();
    parsed.map_err(ManifestFailure::into_codec)
}

fn parse_capability_registry<'ctx>(
    cursor: &mut Cursor<'_, 'ctx, '_>,
) -> Result<(), ManifestFailure<'ctx>> {
    let mut storage = cursor
        .ctx
        .reserve_scoped(0, "F3D asset capability scratch")?;
    storage.with_storage(|| {
        let capability_count = cursor.count(
            "Fusion asset manifest capability count",
            MAX_REGISTRY_ENTRIES,
        )?;
        let mut capability_names = BTreeSet::new();
        for ordinal in 0..capability_count {
            let (field, _field_budget) = cursor.ctx.format_scoped(
                format_args!("Fusion asset manifest capability name {ordinal}"),
                "describe F3D manifest field",
            )?;
            let name = cursor.ascii(&field)?;
            cursor.ctx.charge_work(
                u64_from_index(name.len()) * u64_from_index(capability_names.len() + 1) * 2,
                "compare F3D asset capability names",
            )?;
            if name.is_empty() || capability_names.contains(&name) {
                return Err(probe_malformed(
                    cursor.ctx,
                    "Fusion asset manifest capabilities",
                    format_args!("empty or duplicate name {name:?}"),
                ));
            }
            cursor.ctx.insert_btree_set(
                &mut capability_names,
                name,
                "index F3D asset capability names",
            )?;
            let (field, _field_budget) = cursor.ctx.format_scoped(
                format_args!("Fusion asset manifest capability value {ordinal}"),
                "describe F3D manifest field",
            )?;
            let _value = cursor.u32(&field)?;
        }
        Ok(())
    })
}

fn parse_current_design_asset<'a, 'ctx>(
    cursor: &mut Cursor<'a, 'ctx, '_>,
) -> Result<Option<&'a str>, ManifestFailure<'ctx>> {
    parse_capability_registry(cursor)?;
    cursor.expect_ascii("Fusion asset manifest kind", "Neutron3DAssetType")?;
    cursor.expect_u8("Fusion asset manifest subtype mode", 0)?;
    let subtype = cursor.ascii("Fusion asset manifest subtype")?;
    Ok((!subtype.is_empty()).then_some(subtype))
}

fn parse_revision_zero_design_asset<'ctx>(
    cursor: &mut Cursor<'_, 'ctx, '_>,
) -> Result<(), ManifestFailure<'ctx>> {
    cursor.expect_u32("revision-0 Fusion asset schema", 3)?;
    cursor.expect_u32("revision-0 Fusion asset kind count", 1)?;
    cursor.expect_ascii("revision-0 Fusion asset kind", "Neutron3DAssetType")?;
    cursor.expect_u8("revision-0 Fusion asset subtype mode", 0)?;
    cursor.expect_u32("revision-0 Fusion asset subtype", 0)?;
    cursor.expect_u32("revision-0 Fusion asset schema revision", 6)?;
    cursor.expect_u32("revision-0 Fusion asset root marker", 1)?;
    cursor.expect_u32("revision-0 Fusion asset reserved word", 0)?;
    cursor.expect_ascii("revision-0 Fusion asset role", "Design")?;
    cursor.expect_ascii("revision-0 Fusion asset role name", "Design")?;
    Ok(())
}

fn parse_revision_ten_design_asset<'ctx>(
    cursor: &mut Cursor<'_, 'ctx, '_>,
) -> Result<(), ManifestFailure<'ctx>> {
    parse_capability_registry(cursor)?;
    cursor.expect_ascii("revision-10 Fusion asset kind", "Neutron3DAssetType")?;
    cursor.expect_u8("revision-10 Fusion asset subtype mode", 0)?;
    let mut link_count = 0_usize;
    loop {
        let marker = cursor.u32("revision-10 Fusion asset entry marker")?;
        if marker == 0 {
            let _root_revision = cursor.u32("revision-10 Fusion asset root revision")?;
            break;
        }
        if marker != 2 {
            return Err(probe_malformed(
                cursor.ctx,
                "revision-10 Fusion asset entry marker",
                format_args!("expected 0 or 2, found {marker}"),
            ));
        }
        let locator_units = cursor.count(
            "revision-10 Fusion asset locator length or root revision",
            MAX_MANIFEST_STRING_UNITS,
        )?;
        if locator_units == 5 {
            break;
        }
        if link_count == MAX_REGISTRY_ENTRIES {
            return Err(probe_malformed(
                cursor.ctx,
                "revision-10 Fusion asset links",
                format_args!("count exceeds the limit {MAX_REGISTRY_ENTRIES}"),
            ));
        }
        let (field, _field_budget) = cursor.ctx.format_scoped(
            format_args!("revision-10 Fusion asset link {link_count} locator"),
            "describe F3D manifest field",
        )?;
        let locator = cursor.utf16_with_count(locator_units, &field)?;
        let mut last = ['\0'; 4];
        let contains_urn = locator.chars().any(|character| {
            last.rotate_left(1);
            last[3] = character;
            last == ['u', 'r', 'n', ':']
        });
        if !contains_urn {
            return Err(probe_malformed(
                cursor.ctx,
                "revision-10 Fusion asset link locator",
                format_args!("expected an embedded URN, found {locator:?}"),
            ));
        }
        let (field, _field_budget) = cursor.ctx.format_scoped(
            format_args!("revision-10 Fusion asset link {link_count} GUID 1"),
            "describe F3D manifest field",
        )?;
        let _first_guid = cursor.guid(&field)?;
        let (field, _field_budget) = cursor.ctx.format_scoped(
            format_args!("revision-10 Fusion asset link {link_count} GUID 2"),
            "describe F3D manifest field",
        )?;
        let _second_guid = cursor.guid(&field)?;
        link_count += 1;
    }
    cursor.expect_u32("revision-10 Fusion asset root marker", 1)?;
    cursor.expect_u32("revision-10 Fusion asset reserved word", 0)?;
    cursor.expect_ascii("revision-10 Fusion asset role", "Design")?;
    cursor.expect_ascii("revision-10 Fusion asset role name", "Design")?;
    Ok(())
}

/// Encode the current top-level manifest form for a counted asset-folder run.
pub(crate) fn encode_top_level(
    active_asset_guid: &str,
    asset_folder_bases: &[&str],
) -> Result<Vec<u8>, CodecError> {
    validate_guid(active_asset_guid, "top-level manifest active-asset GUID")?;
    if asset_folder_bases.is_empty() || asset_folder_bases.len() > MAX_ASSET_FOLDERS {
        return Err(malformed(
            "top-level manifest asset-folder count",
            format!("invalid count {}", asset_folder_bases.len()),
        ));
    }
    let mut unique = BTreeSet::new();
    for base in asset_folder_bases {
        validate_asset_base(base)?;
        if !unique.insert(*base) {
            return Err(malformed(
                "top-level manifest asset-folder run",
                format!("duplicate base name {base:?}"),
            ));
        }
    }

    let mut out = Vec::new();
    push_ascii(&mut out, TOP_LEVEL_MANIFEST_VERSION)?;
    push_ascii(&mut out, "FusionDocType")?;
    push_utf16(&mut out, ".f3d")?;
    push_utf16(&mut out, "Fusion Document")?;
    push_utf16(&mut out, "A Fusion Document")?;
    push_utf16(&mut out, GENERATED_DOCUMENT_GUID)?;
    push_utf16(&mut out, GENERATED_DOCUMENT_ASSET_GUID)?;
    push_u32(&mut out, 1234);
    push_u32(&mut out, 20);
    push_u32(&mut out, 36);
    push_u32(&mut out, 0x2a40_0040);
    let registry = [
        ("Application", 1),
        ("CAM", 4),
        ("ParaMesh", 8),
        ("SimCommon", 30_005),
        ("SimFEACSObjects", 2),
        ("SimFluidDynamics", 2),
        ("SimStructuralAttributes", 10_002),
    ];
    push_count(
        &mut out,
        registry.len(),
        "top-level manifest registry count",
    )?;
    for (name, value) in registry {
        push_ascii(&mut out, name)?;
        push_u32(&mut out, value);
    }
    push_u32(&mut out, 0);
    out.push(0);
    push_utf16(&mut out, active_asset_guid)?;
    push_count(
        &mut out,
        asset_folder_bases.len(),
        "top-level manifest asset-folder count",
    )?;
    for base in asset_folder_bases {
        push_utf16(&mut out, base)?;
    }
    push_u32(&mut out, 0);
    out.push(1);
    push_utf16(&mut out, "NA_EXPORT")?;
    Ok(out)
}

/// Encode a complete current-generation Design asset manifest.
pub(crate) fn encode_design_asset(
    base_name: &str,
    primary_guid: &str,
) -> Result<Vec<u8>, CodecError> {
    let mut out = encode_asset_header(
        base_name,
        primary_guid,
        GENERATED_ASSET_GUID,
        DESIGN_ASSET_TYPE,
    )?;
    push_u32(&mut out, 20);
    let capabilities = [
        ("Application", 139),
        ("ParaMesh", 13),
        ("Server", 36),
        ("VolField", 4),
    ];
    push_count(&mut out, capabilities.len(), "asset capability count")?;
    for (name, value) in capabilities {
        push_ascii(&mut out, name)?;
        push_u32(&mut out, value);
    }
    push_ascii(&mut out, "Neutron3DAssetType")?;
    out.push(0);
    push_ascii(&mut out, "")?;
    push_u32(&mut out, 1);
    push_ascii(&mut out, "physicalChangeGuid")?;
    push_utf16(&mut out, GENERATED_PHYSICAL_CHANGE_GUID)?;
    push_u32(&mut out, 0);
    push_u32(&mut out, 7);
    let segment_types = [
        "FusionDesignSegmentType",
        "FusionACTSegmentType",
        "FusionBrowserSegmentType",
    ];
    push_count(&mut out, segment_types.len(), "asset segment-type count")?;
    for (ordinal, name) in segment_types.into_iter().enumerate() {
        push_count(&mut out, ordinal, "asset segment-type ordinal")?;
        push_ascii(&mut out, name)?;
        push_ascii(&mut out, name)?;
    }
    Ok(out)
}

/// Encode the framed prefix shared by every per-asset manifest.
pub(crate) fn encode_asset_header(
    base_name: &str,
    primary_guid: &str,
    secondary_guid: &str,
    asset_type: &str,
) -> Result<Vec<u8>, CodecError> {
    validate_asset_base(base_name)?;
    validate_guid(primary_guid, "asset manifest primary GUID")?;
    validate_guid(secondary_guid, "asset manifest secondary GUID")?;
    if !asset_type.ends_with("AssetType") {
        return Err(malformed(
            "asset manifest asset type",
            format!("invalid asset type {asset_type:?}"),
        ));
    }
    let mut out = Vec::new();
    push_utf16(&mut out, base_name)?;
    push_utf16(&mut out, primary_guid)?;
    push_utf16(&mut out, secondary_guid)?;
    push_ascii(&mut out, asset_type)?;
    Ok(out)
}

pub(crate) fn generated_top_level() -> Result<Vec<u8>, CodecError> {
    encode_top_level(GENERATED_ASSET_FOLDER_GUID, &[GENERATED_DESIGN_ASSET_BASE])
}

/// The generated top-level manifest with its version field replaced.
///
/// Nothing else moves: every field after the version is the byte sequence
/// [`generated_top_level`] wrote, so the archive differs from a known-version
/// archive in the declared version alone.
#[cfg(test)]
pub(crate) fn generated_top_level_with_version(version: &str) -> Vec<u8> {
    let bytes = generated_top_level().expect("generated top-level manifest");
    let mut known = Vec::new();
    push_ascii(&mut known, TOP_LEVEL_MANIFEST_VERSION).expect("known version prefix");
    let mut replacement = Vec::new();
    push_ascii(&mut replacement, version).expect("replacement version prefix");
    assert!(bytes.starts_with(&known), "the version field leads");
    [replacement.as_slice(), &bytes[known.len()..]].concat()
}

pub(crate) fn generated_design_asset() -> Result<Vec<u8>, CodecError> {
    encode_design_asset(GENERATED_DESIGN_ASSET_BASE, GENERATED_ASSET_FOLDER_GUID)
}

fn validate_guid(value: &str, field: &str) -> Result<(), CodecError> {
    if !is_guid_hyphenated(value) {
        return Err(malformed(field, format!("invalid GUID {value:?}")));
    }
    Ok(())
}

fn asset_base_is_valid(mut characters: impl Iterator<Item = char>) -> bool {
    let Some(first) = characters.next() else {
        return false;
    };
    let second = characters.next();
    let third = characters.next();
    if first == '.' && (second.is_none() || (second == Some('.') && third.is_none())) {
        return false;
    }
    std::iter::once(first)
        .chain(second)
        .chain(third)
        .chain(characters)
        .all(|character| !matches!(character, '/' | '\\' | '\0'))
}

fn validate_asset_base_view<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    value: Utf16View<'_>,
) -> Result<(), ManifestFailure<'ctx>> {
    if !asset_base_is_valid(value.chars()) {
        return Err(probe_malformed(
            ctx,
            "asset-folder base name",
            format_args!("invalid path component {value:?}"),
        ));
    }
    Ok(())
}

fn validate_asset_base(value: &str) -> Result<(), CodecError> {
    if !asset_base_is_valid(value.chars()) {
        return Err(malformed(
            "asset-folder base name",
            format!("invalid path component {value:?}"),
        ));
    }
    Ok(())
}

fn bounded_nonzero_count<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    value: u32,
    max: usize,
    field: &str,
) -> Result<usize, ManifestFailure<'ctx>> {
    let count = bounded_count(ctx, value, max, field)?;
    if count == 0 {
        return Err(probe_malformed(
            ctx,
            field,
            format_args!("count {count} is outside 1..={max}"),
        ));
    }
    Ok(count)
}

fn bounded_count<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    value: u32,
    max: usize,
    field: &str,
) -> Result<usize, ManifestFailure<'ctx>> {
    let count = usize::try_from(value)
        .map_err(|_| probe_malformed(ctx, field, "count does not fit memory"))?;
    if count > max {
        return Err(probe_malformed(
            ctx,
            field,
            format_args!("count {count} exceeds the limit {max}"),
        ));
    }
    Ok(count)
}

fn push_ascii(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    if value.len() > MAX_MANIFEST_STRING_UNITS
        || !value.bytes().all(|byte| matches!(byte, 0x20..=0x7e))
    {
        return Err(malformed(
            "manifest ASCII string",
            format!("invalid value {value:?}"),
        ));
    }
    push_count(out, value.len(), "manifest ASCII string length")?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_utf16(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    if units.len() > MAX_MANIFEST_STRING_UNITS {
        return Err(malformed(
            "manifest UTF-16 string",
            format!("{} code units exceed the limit", units.len()),
        ));
    }
    push_count(out, units.len(), "manifest UTF-16 string length")?;
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

fn push_count(out: &mut Vec<u8>, value: usize, field: &str) -> Result<(), CodecError> {
    let value = u32::try_from(value).map_err(|_| malformed(field, "value does not fit u32"))?;
    push_u32(out, value);
    Ok(())
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn malformed(field: &str, message: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("F3D {field}: {message}"))
}

fn truncated<'ctx>(ctx: &'ctx DecodeContext<'_>, field: &str) -> ManifestFailure<'ctx> {
    probe_malformed(ctx, field, "truncated")
}

#[cfg(test)]
mod tests;
