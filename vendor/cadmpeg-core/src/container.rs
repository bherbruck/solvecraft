// SPDX-License-Identifier: Apache-2.0
//! Format-independent container entries.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use crate::decode::u64_from_index;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! label_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub enum $name {
            $(
                #[doc = concat!("The `", $label, "` classification.")]
                #[serde(rename = $label)]
                $variant,
            )*
        }

        impl $name {
            /// Returns the stable summary label.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $label,)* }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

label_enum! {
    /// The closed semantic roles of container summary entries.
    ContainerRole {
        AcisBinary => "acis-binary",
        ActiveBodyIndex => "active-body-index",
        Ancillary => "ancillary",
        Arrangements => "arrangements",
        AssetCatalog => "asset-catalog",
        Auxiliary => "auxiliary",
        Block => "block",
        Brep => "brep",
        BrepSmb => "brep-smb",
        BrepSmbh => "brep-smbh",
        BrepText => "brep-text",
        Bulkstream => "bulkstream",
        CacheCell => "cache-cell",
        CompoundStream => "compound-stream",
        DesignConfig => "design-config",
        Directory => "directory",
        DirectoryEntry => "directory-entry",
        DisplayJt => "display-jt",
        Document => "document",
        EntityRecords => "entity-records",
        ExternalReference => "external-reference",
        ExternalReferences => "external-references",
        FastLoadJt => "fast-load-jt",
        FastLoadStructure => "fast-load-structure",
        FinjplSegment => "finjpl-segment",
        GuiDocument => "gui-document",
        Image => "image",
        InFileAnchors => "in-file-anchors",
        Manifest => "manifest",
        MaterialTexture => "material-texture",
        Metadata => "metadata",
        Metastream => "metastream",
        ModelData => "model-data",
        NamedOpaqueStream => "named-opaque-stream",
        NestedArchive => "nested-archive",
        ObjectClass => "object-class",
        OgsCache => "ogs-cache",
        Opaque => "opaque",
        Other => "other",
        Paramesh => "paramesh",
        ParasolidStream => "parasolid-stream",
        PartAttributes => "part-attributes",
        PartPayload => "part-payload",
        Preview => "preview",
        PreviewImage => "preview-image",
        Properties => "properties",
        Protein => "protein",
        ProteinAssets => "protein-assets",
        PsbGeometry => "psb-geometry",
        RetainedTrailingRecords => "retained-trailing-records",
        RootExchange => "root-exchange",
        RseDatabase => "rse-database",
        RseRevisionTable => "rse-revision-table",
        RseSegmentBulk => "rse-segment-bulk",
        RseSegmentMetadata => "rse-segment-metadata",
        RseSegmentRegistry => "rse-segment-registry",
        RseStorage => "rse-storage",
        SaveToggleInfo => "save-toggle-info",
        Section => "section",
        Signature => "signature",
        Storage => "storage",
        Stream => "stream",
        SubsidiaryExchange => "subsidiary-exchange",
        Table => "table",
        Thumbnail => "thumbnail",
    }
}

label_enum! {
    /// How a container spells "these bytes are stored verbatim".
    VerbatimLabel {
        None => "none",
        Stored => "stored",
    }
}

label_enum! {
    /// Compression methods reported by container summaries.
    CompressionMethod {
        Deflate => "deflate",
        Zstd => "zstd",
        Jpeg => "jpeg",
        UnixCompress => "unix-compress",
        Zlib => "zlib",
    }
}

/// What a container reports about the size of a verbatim payload.
///
/// Every value maps to a distinct declared `(stored, expanded)` pair, and an
/// unreported size is absent on the wire, so a reported zero is an ordinary
/// size rather than a second spelling of [`VerbatimSize::Unreported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimSize {
    /// Neither the payload nor its stored span is reported.
    Unreported,
    /// The payload size; its stored span is not reported.
    PayloadOnly(u64),
    /// The stored span; the payload it expands to is not reported.
    ///
    /// The wire pair (`compressed_size` present, `uncompressed_size` absent) is
    /// its only origin: no codec reports a stored span without its payload.
    StoredOnly(u64),
    /// The payload occupies exactly this many stored bytes.
    Exact(u64),
    /// The payload, plus the container framing counted in its stored span.
    Framed(FramedSpan),
}

/// The byte length of a live allocation.
///
/// Rust guarantees no allocation exceeds `isize::MAX` bytes, so a value read
/// off a slice or a `str` carries that bound with it. A declared length read
/// from a document is minted against the same bound, so every value of this
/// type carries it however it arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "u64"))]
#[serde(try_from = "u64", into = "u64")]
pub struct AllocatedLen(usize);

/// Rejection for a declared length no allocation can reach.
const LENGTH_OVER_ALLOCATION: &str = "declared length exceeds the largest live allocation";

impl AllocatedLen {
    /// A declared byte length, absent when it exceeds `isize::MAX`.
    #[must_use]
    pub fn new(bytes: u64) -> Option<Self> {
        if bytes > u64_from_index(isize::MAX.unsigned_abs()) {
            return None;
        }
        Some(Self(usize::try_from(bytes).ok()?))
    }

    /// The length in bytes.
    #[must_use]
    pub fn get(self) -> u64 {
        u64_from_index(self.0)
    }
}

impl TryFrom<u64> for AllocatedLen {
    type Error = &'static str;

    fn try_from(bytes: u64) -> Result<Self, Self::Error> {
        Self::new(bytes).ok_or(LENGTH_OVER_ALLOCATION)
    }
}

impl From<AllocatedLen> for u64 {
    fn from(length: AllocatedLen) -> Self {
        length.get()
    }
}

impl From<&[u8]> for AllocatedLen {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.len())
    }
}

impl From<&str> for AllocatedLen {
    fn from(text: &str) -> Self {
        Self(text.len())
    }
}

/// The widening in [`FramedSpan::from_parts`] is exact on this target, so a
/// payload length reaches the span unchanged.
const _: () = assert!(usize::BITS <= 64);
/// A live allocation is at most `isize::MAX` bytes and a framing at most
/// `u32::MAX`, so their sum is inside `u64`. This is what justifies the
/// unchecked `+` in [`FramedSpan::stored`], and it justifies nothing else.
const _: () = assert!(isize::BITS <= 64);

/// A verbatim payload inside a strictly larger stored span.
///
/// The framing is the declared number and it is non-zero, so a stored span
/// that does not exceed its payload is unrepresentable. The stored span is
/// derived from the pair and never stated beside it, so a span that disagrees
/// with its own payload and framing is unrepresentable too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramedSpan {
    payload: AllocatedLen,
    framing: NonZeroU32,
}

impl FramedSpan {
    /// A `payload`-byte payload wrapped in `framing` bytes of container framing.
    ///
    /// `payload` is the length of a live allocation and so at most `isize::MAX`
    /// and `framing` at most `u32::MAX`, and the module assertions above prove
    /// that sum inside `u64`, so [`Self::stored`] adds it unchecked and this
    /// mint is total.
    #[must_use]
    pub const fn from_parts(payload: AllocatedLen, framing: NonZeroU32) -> Self {
        Self { payload, framing }
    }

    /// Payload size in bytes.
    #[must_use]
    pub fn payload(self) -> u64 {
        self.payload.get()
    }

    /// Stored span in bytes, framing included: at least `payload + 1`, since
    /// the framing is non-zero.
    ///
    /// The payload is a live allocation and the framing a `u32`, which the
    /// module assertions prove sum inside `u64`, so this addition is total for
    /// every span the type can hold.
    #[must_use]
    pub fn stored(self) -> u64 {
        self.payload.get() + u64::from(self.framing.get())
    }
}

/// How one container summary entry stores its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStorage {
    /// A container directory node that holds no bytes of its own.
    Directory,
    /// Bytes stored verbatim, with whatever the container reports about their size.
    Verbatim {
        /// How this container spells verbatim storage.
        label: VerbatimLabel,
        /// What the container reports about the size.
        size: VerbatimSize,
    },
    /// Bytes stored compressed; either size is absent when the codec does not report it.
    ///
    /// The two sizes are independent facts: a compressed stream may occupy
    /// fewer, the same, or more stored bytes than it expands to (an
    /// incompressible deflate member grows), so no pair of reported sizes is
    /// invalid and none is a second spelling of another.
    Compressed {
        /// Compression method.
        method: CompressionMethod,
        /// Stored size in bytes, absent when the codec does not report it.
        stored: Option<u64>,
        /// Expanded size in bytes, absent when the codec does not report it.
        expanded: Option<u64>,
    },
}

impl EntryStorage {
    /// Verbatim bytes occupying exactly `size` stored bytes.
    #[must_use]
    pub const fn verbatim(label: VerbatimLabel, size: u64) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::Exact(size),
        }
    }

    /// Verbatim bytes whose payload is known and whose stored span is not reported.
    #[must_use]
    pub const fn payload_only(label: VerbatimLabel, payload: u64) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::PayloadOnly(payload),
        }
    }

    /// Verbatim bytes whose stored span may include container framing.
    ///
    /// The two numbers are independently declared by the container, so their
    /// order is a fact about the document: a stored span smaller than the
    /// payload is a malformed declaration and is rejected here, at the door
    /// that reads the container, not at the document wire.
    pub fn framed(
        label: VerbatimLabel,
        payload: u64,
        stored_span: u64,
    ) -> Result<Self, &'static str> {
        let Some(framing) = stored_span.checked_sub(payload) else {
            return Err("verbatim container entry stores fewer bytes than it expands to");
        };
        if framing == 0 {
            return Ok(Self::Verbatim {
                label,
                size: VerbatimSize::Exact(stored_span),
            });
        }
        let Some(payload) = AllocatedLen::new(payload) else {
            return Err(LENGTH_OVER_ALLOCATION);
        };
        let Some(framing) = u32::try_from(framing).ok().and_then(NonZeroU32::new) else {
            return Err("verbatim container entry declares more framing than a span can carry");
        };
        Ok(Self::Verbatim {
            label,
            size: VerbatimSize::Framed(FramedSpan::from_parts(payload, framing)),
        })
    }

    /// Verbatim bytes whose stored span is the payload plus `framing` bytes of
    /// container framing the producer knows.
    ///
    /// Total: [`FramedSpan::from_parts`] mints every `(payload, framing)` pair
    /// a live allocation and a `u32` framing can state.
    #[must_use]
    pub fn framed_by(label: VerbatimLabel, payload: AllocatedLen, framing: NonZeroU32) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::Framed(FramedSpan::from_parts(payload, framing)),
        }
    }

    /// Verbatim bytes whose size the container does not report.
    #[must_use]
    pub const fn unreported(label: VerbatimLabel) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::Unreported,
        }
    }

    /// Stored span in bytes, absent when the container reports none.
    #[must_use]
    pub fn stored_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { size, .. } => match size {
                VerbatimSize::Unreported | VerbatimSize::PayloadOnly(_) => None,
                VerbatimSize::StoredOnly(size) | VerbatimSize::Exact(size) => Some(*size),
                VerbatimSize::Framed(span) => Some(span.stored()),
            },
            Self::Compressed { stored, .. } => *stored,
        }
    }

    /// Expanded size in bytes, absent when the container reports none.
    #[must_use]
    pub fn expanded_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { size, .. } => match size {
                VerbatimSize::Unreported | VerbatimSize::StoredOnly(_) => None,
                VerbatimSize::PayloadOnly(payload) | VerbatimSize::Exact(payload) => Some(*payload),
                VerbatimSize::Framed(span) => Some(span.payload()),
            },
            Self::Compressed { expanded, .. } => *expanded,
        }
    }
}

/// One stream or segment in a container summary.
///
/// `role` and `attributes` are codec-defined. The ordered attribute map keeps
/// the format-independent summary deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ContainerEntryWire"))]
#[serde(from = "ContainerEntryWire", into = "ContainerEntryWire")]
pub struct ContainerEntry {
    /// Entry name/path within the container.
    pub name: String,
    /// Codec-defined role classification.
    pub role: ContainerRole,
    /// Byte storage of this entry.
    pub storage: EntryStorage,
    /// Extra codec-extracted attributes, sorted by key.
    #[serde(deserialize_with = "crate::distinct_keys::btree_map")]
    pub attributes: BTreeMap<String, String>,
}

impl ContainerEntry {
    /// Stored span in bytes, absent when the entry reports none.
    #[must_use]
    pub fn stored_size(&self) -> Option<u64> {
        self.storage.stored_size()
    }

    /// Expanded size in bytes, absent when the entry reports none.
    #[must_use]
    pub fn expanded_size(&self) -> Option<u64> {
        self.storage.expanded_size()
    }
}

/// The entry fields every storage kind carries.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct EntryIdentityWire {
    /// Entry name/path within the container.
    name: String,
    /// Codec-defined role classification.
    role: ContainerRole,
    /// Extra codec-extracted attributes, sorted by key.
    #[serde(default)]
    #[serde(deserialize_with = "crate::distinct_keys::btree_map")]
    attributes: BTreeMap<String, String>,
}

/// What a container reports about a compressed entry's two sizes.
///
/// A compressed stream may occupy fewer, the same, or more stored bytes than
/// it expands to, so the two numbers are independent and either may be absent.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CompressedSizesWire {
    /// Stored size in bytes, absent when the codec does not report it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stored"
    )]
    stored: Option<u64>,
    /// Expanded size in bytes, absent when the codec does not report it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expanded"
    )]
    expanded: Option<u64>,
}

/// What a container reports about a verbatim entry's size.
///
/// One tagged object names which sizes the container reported. The stored span
/// of a framed payload is the payload plus the framing, so it is never stated
/// beside them and cannot disagree with them.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum VerbatimSizeWire {
    /// Neither the payload nor its stored span is reported.
    Unreported {},
    /// The payload size alone.
    PayloadOnly {
        /// Payload size in bytes.
        payload: u64,
    },
    /// The stored span alone.
    StoredOnly {
        /// Stored span in bytes.
        stored: u64,
    },
    /// The payload occupies exactly this many stored bytes.
    Exact {
        /// Payload size and stored span in bytes.
        size: u64,
    },
    /// The payload, plus the container framing counted in its stored span.
    Framed {
        /// Payload size in bytes.
        payload: AllocatedLen,
        /// Framing bytes counted in the stored span but not in the payload.
        framing: NonZeroU32,
    },
}

impl From<VerbatimSize> for VerbatimSizeWire {
    fn from(size: VerbatimSize) -> Self {
        match size {
            VerbatimSize::Unreported => Self::Unreported {},
            VerbatimSize::PayloadOnly(payload) => Self::PayloadOnly { payload },
            VerbatimSize::StoredOnly(stored) => Self::StoredOnly { stored },
            VerbatimSize::Exact(size) => Self::Exact { size },
            VerbatimSize::Framed(span) => Self::Framed {
                payload: span.payload,
                framing: span.framing,
            },
        }
    }
}

impl From<VerbatimSizeWire> for VerbatimSize {
    fn from(wire: VerbatimSizeWire) -> Self {
        match wire {
            VerbatimSizeWire::Unreported {} => Self::Unreported,
            VerbatimSizeWire::PayloadOnly { payload } => Self::PayloadOnly(payload),
            VerbatimSizeWire::StoredOnly { stored } => Self::StoredOnly(stored),
            VerbatimSizeWire::Exact { size } => Self::Exact(size),
            VerbatimSizeWire::Framed { payload, framing } => {
                Self::Framed(FramedSpan::from_parts(payload, framing))
            }
        }
    }
}

/// One container summary entry, tagged by how it stores its bytes.
///
/// The `storage` spelling is a directory node, which holds no bytes of its
/// own, so it carries no size keys: a directory that declares a byte size is
/// unrepresentable rather than refused.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "compression")]
#[serde(deny_unknown_fields)]
enum ContainerEntryWire {
    /// A container directory node that holds no bytes of its own.
    #[serde(rename = "storage")]
    Directory {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
    },
    /// Bytes stored verbatim, which this container spells `none`.
    #[serde(rename = "none")]
    VerbatimNone {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the size.
        size: VerbatimSizeWire,
    },
    /// Bytes stored verbatim, which this container spells `stored`.
    #[serde(rename = "stored")]
    VerbatimStored {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the size.
        size: VerbatimSizeWire,
    },
    /// Bytes stored compressed with deflate.
    #[serde(rename = "deflate")]
    Deflate {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the entry's two sizes.
        #[serde(flatten)]
        sizes: CompressedSizesWire,
    },
    /// Bytes stored compressed with zstd.
    #[serde(rename = "zstd")]
    Zstd {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the entry's two sizes.
        #[serde(flatten)]
        sizes: CompressedSizesWire,
    },
    /// Bytes stored compressed with JPEG.
    #[serde(rename = "jpeg")]
    Jpeg {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the entry's two sizes.
        #[serde(flatten)]
        sizes: CompressedSizesWire,
    },
    /// Bytes stored compressed with Unix compress.
    #[serde(rename = "unix-compress")]
    UnixCompress {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the entry's two sizes.
        #[serde(flatten)]
        sizes: CompressedSizesWire,
    },
    /// Bytes stored compressed with zlib.
    #[serde(rename = "zlib")]
    Zlib {
        /// The entry fields every storage kind carries.
        #[serde(flatten)]
        identity: EntryIdentityWire,
        /// What the container reports about the entry's two sizes.
        #[serde(flatten)]
        sizes: CompressedSizesWire,
    },
}

impl From<ContainerEntry> for ContainerEntryWire {
    fn from(entry: ContainerEntry) -> Self {
        let identity = EntryIdentityWire {
            name: entry.name,
            role: entry.role,
            attributes: entry.attributes,
        };
        match entry.storage {
            EntryStorage::Directory => Self::Directory { identity },
            EntryStorage::Verbatim { label, size } => {
                let size = size.into();
                match label {
                    VerbatimLabel::None => Self::VerbatimNone { identity, size },
                    VerbatimLabel::Stored => Self::VerbatimStored { identity, size },
                }
            }
            EntryStorage::Compressed {
                method,
                stored,
                expanded,
            } => {
                let sizes = CompressedSizesWire { stored, expanded };
                match method {
                    CompressionMethod::Deflate => Self::Deflate { identity, sizes },
                    CompressionMethod::Zstd => Self::Zstd { identity, sizes },
                    CompressionMethod::Jpeg => Self::Jpeg { identity, sizes },
                    CompressionMethod::UnixCompress => Self::UnixCompress { identity, sizes },
                    CompressionMethod::Zlib => Self::Zlib { identity, sizes },
                }
            }
        }
    }
}

impl From<ContainerEntryWire> for ContainerEntry {
    fn from(wire: ContainerEntryWire) -> Self {
        let verbatim = |label: VerbatimLabel, size: VerbatimSizeWire| EntryStorage::Verbatim {
            label,
            size: size.into(),
        };
        let compressed =
            |method: CompressionMethod, sizes: CompressedSizesWire| EntryStorage::Compressed {
                method,
                stored: sizes.stored,
                expanded: sizes.expanded,
            };
        let (identity, storage) = match wire {
            ContainerEntryWire::Directory { identity } => (identity, EntryStorage::Directory),
            ContainerEntryWire::VerbatimNone { identity, size } => {
                (identity, verbatim(VerbatimLabel::None, size))
            }
            ContainerEntryWire::VerbatimStored { identity, size } => {
                (identity, verbatim(VerbatimLabel::Stored, size))
            }
            ContainerEntryWire::Deflate { identity, sizes } => {
                (identity, compressed(CompressionMethod::Deflate, sizes))
            }
            ContainerEntryWire::Zstd { identity, sizes } => {
                (identity, compressed(CompressionMethod::Zstd, sizes))
            }
            ContainerEntryWire::Jpeg { identity, sizes } => {
                (identity, compressed(CompressionMethod::Jpeg, sizes))
            }
            ContainerEntryWire::UnixCompress { identity, sizes } => {
                (identity, compressed(CompressionMethod::UnixCompress, sizes))
            }
            ContainerEntryWire::Zlib { identity, sizes } => {
                (identity, compressed(CompressionMethod::Zlib, sizes))
            }
        };
        Self {
            name: identity.name,
            role: identity.role,
            storage,
            attributes: identity.attributes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::u64_from_index;
    use super::{
        CompressionMethod, ContainerEntry, ContainerRole, EntryStorage, VerbatimLabel, VerbatimSize,
    };
    use std::collections::BTreeMap;
    use std::num::{NonZeroU32, NonZeroU64};

    fn verbatim_wire(compression: &str, size: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": compression,
            "size": size.clone(),
        })
    }

    fn compressed_wire(
        compression: &str,
        stored: Option<u64>,
        expanded: Option<u64>,
    ) -> serde_json::Value {
        let mut value = serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": compression,
        });
        if let Some(stored) = stored {
            value["stored"] = stored.into();
        }
        if let Some(expanded) = expanded {
            value["expanded"] = expanded.into();
        }
        value
    }

    fn admit(wire: serde_json::Value) -> ContainerEntry {
        serde_json::from_value(wire).expect("the declared sizes are admissible")
    }

    fn reject(wire: serde_json::Value) -> String {
        serde_json::from_value::<ContainerEntry>(wire)
            .expect_err("the declared sizes are inadmissible")
            .to_string()
    }

    fn nonzero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("a nonzero test size")
    }

    fn entry(storage: EntryStorage) -> ContainerEntry {
        ContainerEntry {
            name: "entry".to_string(),
            role: ContainerRole::Stream,
            storage,
            attributes: BTreeMap::new(),
        }
    }

    fn framed(payload: u64, framing: u32) -> VerbatimSize {
        VerbatimSize::Framed(super::FramedSpan::from_parts(
            super::AllocatedLen::new(payload).expect("a live payload length"),
            NonZeroU32::new(framing).expect("a nonzero test framing"),
        ))
    }

    #[test]
    fn each_declared_verbatim_size_has_one_spelling() {
        for (size, expected) in [
            (
                serde_json::json!({"form": "unreported"}),
                VerbatimSize::Unreported,
            ),
            (
                serde_json::json!({"form": "payload_only", "payload": 12}),
                VerbatimSize::PayloadOnly(12),
            ),
            (
                serde_json::json!({"form": "payload_only", "payload": 0}),
                VerbatimSize::PayloadOnly(0),
            ),
            (
                serde_json::json!({"form": "stored_only", "stored": 5}),
                VerbatimSize::StoredOnly(5),
            ),
            (
                serde_json::json!({"form": "exact", "size": 7}),
                VerbatimSize::Exact(7),
            ),
            (
                serde_json::json!({"form": "exact", "size": 0}),
                VerbatimSize::Exact(0),
            ),
            (
                serde_json::json!({"form": "framed", "payload": 10, "framing": 20}),
                framed(10, 20),
            ),
            (
                serde_json::json!({"form": "framed", "payload": 0, "framing": 5}),
                framed(0, 5),
            ),
        ] {
            let storage = EntryStorage::Verbatim {
                label: VerbatimLabel::None,
                size: expected,
            };
            let read = admit(verbatim_wire("none", &size));
            assert_eq!(read.storage, storage, "{size}");
            let mut expected_wire = verbatim_wire("none", &size);
            expected_wire["attributes"] = serde_json::json!({});
            assert_eq!(
                serde_json::to_value(entry(storage)).expect("a container entry serializes"),
                expected_wire,
                "{size}"
            );
        }
    }

    #[test]
    fn a_framed_payload_carries_no_stored_span_to_disagree_with() {
        let error = reject(verbatim_wire(
            "none",
            &serde_json::json!({"form": "framed", "payload": 9, "framing": 1, "stored": 5}),
        ));
        assert!(error.contains("unknown field"), "{error}");
        assert!(error.contains("stored"), "{error}");

        let error = reject(verbatim_wire(
            "none",
            &serde_json::json!({"form": "framed", "payload": 9, "framing": 0}),
        ));
        assert!(error.contains("zero"), "{error}");

        let error = reject(verbatim_wire(
            "none",
            &serde_json::json!({"form": "framed", "payload": u64::MAX, "framing": 1}),
        ));
        assert!(
            error.contains("declared length exceeds the largest live allocation"),
            "{error}"
        );
    }

    #[test]
    fn a_container_door_refuses_a_span_under_its_payload() {
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 9, 5),
            Err("verbatim container entry stores fewer bytes than it expands to")
        );
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 9, 9),
            Ok(EntryStorage::verbatim(VerbatimLabel::Stored, 9))
        );
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 10, 30),
            Ok(EntryStorage::Verbatim {
                label: VerbatimLabel::Stored,
                size: framed(10, 20),
            })
        );
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 0, u64::from(u32::MAX) + 1),
            Err("verbatim container entry declares more framing than a span can carry")
        );
    }

    #[test]
    fn compressed_sizes_spell_unreported_only_as_absence() {
        for (stored, expanded) in [
            (None, None),
            (None, Some(99)),
            (Some(44), None),
            (Some(4), Some(16)),
            (Some(0), Some(0)),
        ] {
            let storage = EntryStorage::Compressed {
                method: CompressionMethod::Deflate,
                stored,
                expanded,
            };
            let mut expected = compressed_wire("deflate", stored, expanded);
            expected["attributes"] = serde_json::json!({});
            let serialized =
                serde_json::to_value(entry(storage.clone())).expect("a container entry serializes");
            assert_eq!(serialized, expected);
            let read_back: ContainerEntry =
                serde_json::from_value(serialized).expect("the wire value re-admits");
            assert_eq!(read_back.storage, storage);
        }
    }

    #[test]
    fn a_minted_framed_span_always_exceeds_its_payload() {
        let empty: &[u8] = &[];
        let span = super::FramedSpan::from_parts(empty.into(), NonZeroU32::MAX);
        assert_eq!(span.payload(), 0);
        assert_eq!(
            nonzero(span.stored() - span.payload()),
            nonzero(u64::from(u32::MAX))
        );
        assert_eq!(span.stored(), u64::from(u32::MAX));
        assert!(span.stored() > span.payload());
        let span = super::FramedSpan::from_parts(empty.into(), NonZeroU32::MIN);
        assert_eq!(span.stored(), 1);
        let body = vec![0u8; 12];
        let span = super::FramedSpan::from_parts(body.as_slice().into(), NonZeroU32::MIN);
        assert_eq!(span.payload(), u64_from_index(body.len()));
        assert_eq!(span.stored(), 13);
        assert_eq!(nonzero(span.stored() - span.payload()), nonzero(1));
        let span = super::FramedSpan::from_parts("target.CATPart".into(), NonZeroU32::MAX);
        assert_eq!(span.payload(), u64_from_index("target.CATPart".len()));
        assert_eq!(
            span.stored(),
            u64_from_index("target.CATPart".len()) + u64::from(u32::MAX)
        );
    }

    #[test]
    fn a_reported_zero_size_is_not_an_unreported_one() {
        let reported = admit(verbatim_wire(
            "none",
            &serde_json::json!({"form": "exact", "size": 0}),
        ));
        let unreported = admit(verbatim_wire(
            "none",
            &serde_json::json!({"form": "unreported"}),
        ));
        assert_ne!(reported.storage, unreported.storage);
        assert_eq!(reported.stored_size(), Some(0));
        assert_eq!(reported.expanded_size(), Some(0));
        assert_eq!(unreported.stored_size(), None);
        assert_eq!(unreported.expanded_size(), None);
        assert_ne!(
            EntryStorage::verbatim(VerbatimLabel::None, 0),
            EntryStorage::unreported(VerbatimLabel::None)
        );
    }

    #[test]
    fn a_storage_label_cannot_declare_a_byte_size() {
        let error = reject(verbatim_wire(
            "storage",
            &serde_json::json!({"form": "exact", "size": 4}),
        ));
        assert!(error.contains("unknown field"), "{error}");
        assert!(error.contains("size"), "{error}");
        let error = reject(compressed_wire("storage", Some(4), Some(7)));
        assert!(error.contains("unknown field"), "{error}");

        let entry: ContainerEntry = serde_json::from_value(serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": "storage",
        }))
        .expect("a directory node declares no size");
        assert_eq!(entry.storage, EntryStorage::Directory);
    }

    #[test]
    fn reported_sizes_are_absent_rather_than_zero() {
        let payload_only = admit(verbatim_wire(
            "none",
            &serde_json::json!({"form": "payload_only", "payload": 12}),
        ));
        assert_eq!(payload_only.stored_size(), None);
        assert_eq!(payload_only.expanded_size(), Some(12));
        let framed_entry = admit(verbatim_wire(
            "none",
            &serde_json::json!({"form": "framed", "payload": 10, "framing": 20}),
        ));
        assert_eq!(framed_entry.stored_size(), Some(30));
        assert_eq!(framed_entry.expanded_size(), Some(10));
        let compressed = admit(compressed_wire("zlib", None, Some(99)));
        assert_eq!(compressed.stored_size(), None);
        assert_eq!(compressed.expanded_size(), Some(99));
    }

    #[test]
    fn a_container_entry_refuses_an_unknown_key_beside_its_name() {
        let wire = serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": "none",
            "size": {"form": "unreported"},
        });
        serde_json::from_value::<ContainerEntry>(wire.clone()).expect("a legal entry");

        let mut stray = wire;
        stray["zz_bogus"] = serde_json::json!(1);
        let error = serde_json::from_value::<ContainerEntry>(stray)
            .expect_err("an unknown key has no encoding")
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }
}

// Each optional key below names itself in whatever it refuses.
crate::named_optional_field!(deserialize_stored, u64, "stored");
crate::named_optional_field!(deserialize_expanded, u64, "expanded");
