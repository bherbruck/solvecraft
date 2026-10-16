// SPDX-License-Identifier: Apache-2.0
//! The `f3d:` URN identifier scheme.
//!
//! Segment vocabulary, separators, ordering, escaping, and `#{len}:{key}`
//! length-prefixes. Callers build IDs through the named functions below.

use crate::records::{
    feature::combine::DesignCombineExternalBodyIdentity, parameters::DesignParameter,
    sketch_placement::DesignSketchPlacement,
};

#[cfg(test)]
use crate::records::feature::{
    assembly::{DesignAssemblyAxialSelectorIdentity, DesignAssemblyLegacySelection},
    scope::DesignParameterScope,
};

/// The scheme prefix shared by every `f3d:` URN. Used to strip or test the
/// scheme when parsing an identity key back into its stream and tail.
pub(crate) const SCHEME_PREFIX: &str = "f3d:";

/// Format component of every entity ID this codec emits.
pub(crate) const ID_FORMAT: cadmpeg_asm::ids::IdFormat = cadmpeg_asm::asm_format!("f3d");

/// The native stream used when an identity key carries no qualifying stream —
/// the fallback for `native_stream(id).unwrap_or(..)`.
pub(crate) const DEFAULT_STREAM: &str = "f3d:design";

/// Parse the native stream segment out of an identity key: the text before the
/// final `:` separator. Returns `None` when the key carries no separator.
pub(crate) fn native_stream(id: &str) -> Option<&str> {
    id.rsplit_once(':').map(|(stream, _)| stream)
}

/// Return whether two native IDs belong to the same root document or xref occurrence.
pub(crate) fn same_native_occurrence(left: &str, right: &str) -> bool {
    const OCCURRENCE_SEGMENT: &str = "/occurrence-";

    fn occurrence(id: &str) -> Option<&str> {
        let mut occurrence_end = None;
        for (at, _) in id.match_indices(OCCURRENCE_SEGMENT) {
            let digits_at = at + OCCURRENCE_SEGMENT.len();
            let end = id[digits_at..]
                .find('/')
                .map_or(id.len(), |end| digits_at + end);
            if end > digits_at && id[digits_at..end].bytes().all(|byte| byte.is_ascii_digit()) {
                occurrence_end = Some(end);
            }
        }
        occurrence_end.map(|end| &id[..end])
    }

    match (occurrence(left), occurrence(right)) {
        (Some(left), Some(right)) => left == right,
        (None, None) => !left.contains(OCCURRENCE_SEGMENT) && !right.contains(OCCURRENCE_SEGMENT),
        _ => false,
    }
}

/// Parse the Design segment shared by sibling `MetaStream.dat` and
/// `BulkStream.dat` entries from a native record identity.
pub(crate) fn design_segment(id: &str) -> Option<&str> {
    let stream = native_stream(id)?;
    let (segment, entry) = stream.rsplit_once('/')?;
    matches!(entry, "MetaStream.dat" | "BulkStream.dat").then_some(segment)
}

/// The fixed key of the single source-image record a design carries.
pub(crate) const FILE_SOURCE_IMAGE_ID: &str = "f3d:file:source-image#0";

/// The fixed identity of the retained source image record.
pub(crate) fn file_source_image_id() -> cadmpeg_ir::ids::UnknownId {
    cadmpeg_ir::ids::UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "file", "source-image"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// The neutral B-rep identity for one face slot.
pub(crate) fn brep_face_id(
    index: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::FaceId {
    cadmpeg_ir::ids::FaceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "brep", "entity"),
        index,
    )
}

/// Build an appearance identity from its source visual token.
#[cfg(test)]
pub(crate) fn appearance_id(key: cadmpeg_ir::ids::IdentityKey) -> cadmpeg_ir::ids::AppearanceId {
    cadmpeg_ir::ids::AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "appearance"),
        key,
    )
}

pub(crate) fn appearance_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    key: &str,
) -> Result<cadmpeg_ir::ids::AppearanceId, cadmpeg_core::CodecError> {
    let id = native_scoped_id_charged(ctx, "design", "appearance", key)?;
    cadmpeg_ir::ids::AppearanceId::mint(id).map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "F3D appearance identity is invalid: {error}"
        ))
    })
}

/// Build a body appearance binding identity.
#[cfg(test)]
pub(crate) fn body_appearance_binding_id(
    entity_suffix: u64,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
) -> cadmpeg_ir::ids::AppearanceBindingId {
    let key = cadmpeg_ir::ids::IdentityKey::from(entity_suffix).colon(visual_guid);
    cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "body"),
        key,
    )
}

pub(crate) fn body_appearance_binding_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity_suffix: u64,
    visual_guid: &str,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_core::CodecError> {
    let id = native_scoped_id_charged(
        ctx,
        "appearance",
        "body",
        format_args!("{entity_suffix}:{visual_guid}"),
    )?;
    cadmpeg_ir::ids::AppearanceBindingId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

/// Build a body assignment appearance binding identity.
#[cfg(test)]
pub(crate) fn assignment_appearance_binding_id(
    entity_id: &str,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_ir::ids::IdentityError> {
    let entity = cadmpeg_ir::ids::IdentityKeyTail::try_new(entity_id)?;
    let key = cadmpeg_ir::identity_key!(":")
        .with_prefix(&entity)
        .then(visual_guid);
    Ok(cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "binding"),
        key,
    ))
}

pub(crate) fn assignment_appearance_binding_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity_id: &str,
    visual_guid: &str,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_core::CodecError> {
    let id = native_scoped_id_charged(
        ctx,
        "appearance",
        "binding",
        format_args!("{entity_id}:{visual_guid}"),
    )?;
    cadmpeg_ir::ids::AppearanceBindingId::mint(id).map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "F3D appearance binding identity is invalid: {error}"
        ))
    })
}

/// Build a face appearance binding identity.
#[cfg(test)]
pub(crate) fn face_appearance_binding_id(
    face_guid: &str,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_ir::ids::IdentityError> {
    let face_guid = cadmpeg_ir::ids::IdentityKeyTail::try_new(face_guid)?;
    let face_key = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(face.as_str());
    let key = cadmpeg_ir::identity_key!(":")
        .with_prefix(&face_guid)
        .then(visual_guid)
        .colon(face_key.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&face_key);
    Ok(cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "face"),
        key,
    ))
}

fn write_escaped_identity_component(
    formatter: &mut std::fmt::Formatter<'_>,
    source: &str,
) -> std::fmt::Result {
    for character in source.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                write!(formatter, "%{byte:02X}")?;
            }
        } else {
            write!(formatter, "{character}")?;
        }
    }
    Ok(())
}

struct FaceBindingKey<'a> {
    face_guid: &'a str,
    visual_guid: &'a str,
    face: &'a str,
    escaped_face_len: usize,
}

impl std::fmt::Display for FaceBindingKey<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}:{}:{}:",
            self.face_guid, self.visual_guid, self.escaped_face_len
        )?;
        write_escaped_identity_component(formatter, self.face)
    }
}

/// Compose a face appearance binding ID with its caller's decode budget.
pub(crate) fn face_appearance_binding_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    face_guid: &str,
    visual_guid: &crate::records::references::DesignVisualToken,
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_core::CodecError> {
    let escaped_face_len =
        escaped_scope_len(ctx, face.as_str(), "retain F3D face appearance binding ID")?;
    let id = native_scoped_id_charged(
        ctx,
        "appearance",
        "face",
        FaceBindingKey {
            face_guid,
            visual_guid,
            face: face.as_str(),
            escaped_face_len,
        },
    )?;
    cadmpeg_ir::ids::AppearanceBindingId::mint(id).map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "F3D face appearance binding identity is invalid: {error}"
        ))
    })
}

/// Build a T-spline identity from its source entry key.
#[cfg(test)]
pub(crate) fn subd_id(
    source_key: &str,
) -> Result<cadmpeg_ir::ids::SubdId, cadmpeg_ir::ids::IdentityError> {
    let key = cadmpeg_ir::ids::IdentityKey::try_new(source_key)?;
    Ok(cadmpeg_ir::ids::SubdId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "tspline", "subd"),
        key,
    ))
}

/// Percent-encode identity separators, the escape byte, and whitespace.
pub(crate) fn identity_key_component(value: &str) -> String {
    cadmpeg_ir::ids::IdentityKeyTail::percent_encode(value)
        .as_str()
        .to_owned()
}

fn identity_key_component_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    let operation = "retain F3D Combine identity component";
    let mut length = 0usize;
    for character in value.chars() {
        let width = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            character.len_utf8().checked_mul(3)
        } else {
            Some(character.len_utf8())
        };
        length = length
            .checked_add(width.ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    }
    let mut encoded = ctx.retained_string(length, operation)?;
    for character in value.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            let mut scalar = [0; 4];
            for byte in character.encode_utf8(&mut scalar).as_bytes() {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            encoded.push(character);
        }
    }
    Ok(encoded)
}

/// Reverse [`identity_key_component`] for a complete encoded component.
pub(crate) fn decode_identity_key_component(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let bytes = value.as_bytes();
    let mut decoded = ctx.vector_storage(bytes.len(), "decode F3D identity key bytes")?;
    Ok((|| {
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] != b'%' {
                decoded.push(bytes[at]);
                at += 1;
                continue;
            }
            let pair = bytes.get(at + 1..at + 3)?;
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            decoded.push(digit(pair[0])? * 16 + digit(pair[1])?);
            at += 3;
        }
        String::from_utf8(decoded).ok()
    })())
}

/// The neutral B-rep topology entity key for entity `index`.
pub(crate) fn brep_entity_id(index: impl std::fmt::Display) -> String {
    format!("f3d:brep:entity#{index}")
}

/// Neutral product occurrence projected from one external-reference placement.
pub(crate) fn neutral_xref_occurrence_id(
    reference_ordinal: u32,
    occurrence_ordinal: u32,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::identity_key!("xref")
            .dash(reference_ordinal)
            .dash(occurrence_ordinal),
    )
}

/// Neutral local component definition projected from its stable Design GUID.
pub(crate) fn neutral_component_id(
    guid: &crate::records::mesh::DesignRelaxedGuidText,
) -> cadmpeg_ir::ids::ProductDefinitionId {
    cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "component"),
        guid.identity_key(),
    )
}

/// Neutral local occurrence projected from its stable Design GUID.
pub(crate) fn neutral_component_occurrence_id(
    guid: &crate::records::mesh::DesignRelaxedGuidText,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        guid.identity_key(),
    )
}

/// Neutral occurrence identity for an external component-insert scope whose
/// target document is not present in the container.
#[cfg(test)]
pub(crate) fn neutral_component_insert_occurrence_id(
    scope: &DesignParameterScope,
) -> cadmpeg_ir::ids::OccurrenceId {
    let stream = identity_key_component(native_stream(&scope.id).unwrap_or(DEFAULT_STREAM));
    let stream_len = stream.len();
    let key = cadmpeg_ir::identity_key!("component-insert-")
        .then(stream_len)
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
            native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
        ))
        .then(scope.feature_ordinal.get())
        .colon(scope.record_index);
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        key,
    )
}

/// Neutral assembly-joint key projected from one Design parameter scope.
#[cfg(test)]
pub(crate) fn neutral_assembly_joint_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::products::JointId, cadmpeg_core::CodecError> {
    struct JointKey<'a> {
        stream: &'a str,
        encoded_len: usize,
        record_index: u32,
    }
    impl std::fmt::Display for JointKey<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{}:", self.encoded_len)?;
            write_escaped_identity_component(formatter, self.stream)?;
            write!(formatter, "{}", self.record_index)
        }
    }
    let stream = native_stream(&scope.id).unwrap_or(DEFAULT_STREAM);

    let encoded_len = escaped_scope_len(ctx, stream, "retain F3D neutral joint ID")?;
    let id = native_scoped_id_charged(
        ctx,
        "model",
        "joint",
        JointKey {
            stream,
            encoded_len,
            record_index: scope.record_index,
        },
    )?;
    cadmpeg_ir::products::JointId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

#[cfg(test)]
pub(crate) fn feature_input_topology_id(
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    let feature_key = feature_id.key();
    history_input_state_id(&history_input_prefix(&feature_key, previous_state_id))
}

/// The Design configuration record key for the archive entry `entry_name`.
pub(crate) fn configuration_entry_id(
    entry_name: &str,
    scope: &cadmpeg_ir::ids::IdentityComponent,
) -> String {
    format!(
        "f3d:{}:entry#{}",
        scope.as_str(),
        identity_key_component(entry_name)
    )
}

pub(crate) fn configuration_entry_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entry_name: &str,
    scope: &cadmpeg_ir::ids::IdentityComponent,
) -> Result<String, cadmpeg_core::CodecError> {
    struct EscapedEntry<'a>(&'a str);
    impl std::fmt::Display for EscapedEntry<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write_escaped_identity_component(formatter, self.0)
        }
    }
    ctx.format_retained(
        format_args!("f3d:{}:entry#{}", scope.as_str(), EscapedEntry(entry_name)),
        "retain F3D configuration entry ID",
    )
}

/// The neutral configuration key for `variant_name` under `entry_name`, with
/// both names length-prefixed into `#{len}:{key}{len}:{key}` segments.
#[cfg(test)]
pub(crate) fn neutral_configuration_id(
    entry_name: &str,
    variant_name: &str,
) -> cadmpeg_ir::features::ConfigurationId {
    let entry_name = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(entry_name);
    let variant_name = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(variant_name);
    let key = cadmpeg_ir::ids::IdentityKey::from(entry_name.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&entry_name)
        .then(variant_name.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&variant_name);
    cadmpeg_ir::features::ConfigurationId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "configuration", "variant"),
        key,
    )
}

/// The neutral feature key for a parameter `scope`.
#[cfg(test)]
pub(crate) fn neutral_feature_id(scope: &DesignParameterScope) -> cadmpeg_ir::features::FeatureId {
    neutral_feature_id_parts(
        native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
        scope.kind_name(),
        scope.feature_ordinal.get(),
        scope.record_index,
    )
}

/// The neutral feature key from its `stream`, `kind`, ordinal, and scope record
/// index, with `stream` and `kind` length-prefixed into `#{len}:{key}` segments.
#[cfg(test)]
pub(crate) fn neutral_feature_id_parts(
    stream: &str,
    kind: &str,
    feature_ordinal: u32,
    scope_record_index: u32,
) -> cadmpeg_ir::features::FeatureId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream);
    let kind = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(kind);
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(kind.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&kind)
        .then(feature_ordinal)
        .colon(scope_record_index);
    cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "feature"),
        key,
    )
}

/// Feature-input-local body key for one complete external `Combine` selector path.
pub(crate) fn neutral_combine_external_body_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    identity: &DesignCombineExternalBodyIdentity,
) -> Result<String, cadmpeg_core::CodecError> {
    let selector_asset =
        identity_key_component_charged(ctx, identity.selector_asset_id().as_str())?;
    let selector_context =
        identity_key_component_charged(ctx, identity.selector_context_id().as_str())?;
    let external_asset =
        identity_key_component_charged(ctx, identity.external_asset_id().as_str())?;
    let link_name = identity_key_component_charged(ctx, identity.external_link_name())?;
    let property_key = identity
        .external_version()
        .map(|version| identity_key_component_charged(ctx, version.property_key.value.as_str()))
        .transpose()?
        .unwrap_or_default();
    let version_urn = identity
        .external_version()
        .map(|version| identity_key_component_charged(ctx, version.version_urn.value.as_str()))
        .transpose()?
        .unwrap_or_default();
    ctx.format_retained(format_args!(
            "f3d:feature-input:body#combine-external:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            selector_asset.len(), selector_asset, selector_context.len(), selector_context,
            identity.occurrence_reference(), identity.external_body_reference(),
            identity.external_segment(), external_asset.len(), external_asset,
            link_name.len(), link_name, u8::from(identity.external_version().is_some()),
            property_key.len(), property_key,
            u8::from(identity.external_version().is_some()), version_urn.len(), version_urn,
        ), "retain F3D Combine external body identity")
}

/// Feature-input-local connector key for one pathless axial assembly selector.
#[cfg(test)]
pub(crate) fn neutral_assembly_axial_object_id(
    identity: &DesignAssemblyAxialSelectorIdentity,
) -> String {
    let selector_asset =
        identity_key_component(&identity.selector_asset_id.as_str().to_ascii_lowercase());
    let selector_context =
        identity_key_component(&identity.selector_context_id.as_str().to_ascii_lowercase());
    let external_asset =
        identity_key_component(&identity.external_asset_id.as_str().to_ascii_lowercase());
    let link_name = identity_key_component(&identity.external_link_name);
    let property_key = identity
        .external_version
        .as_ref()
        .map(|version| version.property_key.value.as_str())
        .map(|value| identity_key_component(&value.to_ascii_lowercase()))
        .unwrap_or_default();
    let version_urn = identity
        .external_version
        .as_ref()
        .map(|version| version.version_urn.value.as_str())
        .map(identity_key_component)
        .unwrap_or_default();
    format!(
        "f3d:feature-input:connector#assembly-axial:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        selector_asset.len(),
        selector_asset,
        selector_context.len(),
        selector_context,
        identity.external_object_reference,
        identity.external_segment,
        external_asset.len(),
        external_asset,
        link_name.len(),
        link_name,
        u8::from(identity.external_version.is_some()),
        property_key.len(),
        property_key,
        u8::from(identity.external_version.is_some()),
        version_urn.len(),
        version_urn,
    )
}

/// Feature-input-local connector key for one direct legacy `As-built` face
/// selection.
#[cfg(test)]
pub(crate) fn neutral_assembly_legacy_object_id(
    selection: &DesignAssemblyLegacySelection,
) -> String {
    let asset = identity_key_component(&selection.asset_id.as_str().to_ascii_lowercase());
    let context = identity_key_component(&selection.context_id.as_str().to_ascii_lowercase());
    let recipe = identity_key_component(&selection.recipe_id.to_ascii_lowercase());
    format!(
        "f3d:feature-input:connector#assembly-legacy:{}:{}:{}:{}:{}:{}:{}:{}",
        asset.len(),
        asset,
        context.len(),
        context,
        recipe.len(),
        recipe,
        selection.record_index,
        selection.recipe_record_index,
    )
}

/// The neutral embedded-asset key for one exact archive entry.
#[cfg(test)]
pub(crate) fn neutral_asset_id(entry_name: &str) -> cadmpeg_ir::assets::AssetId {
    let entry_name = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(entry_name);
    let key = cadmpeg_ir::ids::IdentityKey::from(entry_name.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&entry_name);
    cadmpeg_ir::assets::AssetId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "asset"),
        key,
    )
}

/// The neutral parameter key for a design `parameter`.
#[cfg(test)]
pub(crate) fn neutral_parameter_id(
    parameter: &DesignParameter,
) -> cadmpeg_ir::features::ParameterId {
    neutral_parameter_id_parts(
        native_stream(&parameter.id).unwrap_or(DEFAULT_STREAM),
        parameter.record_index,
    )
}

struct EncodedParameterKey<'a> {
    stream: &'a str,
    encoded_len: usize,
    record_index: u32,
}

impl std::fmt::Display for EncodedParameterKey<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:", self.encoded_len)?;
        write_escaped_identity_component(formatter, self.stream)?;
        write!(formatter, "{}", self.record_index)
    }
}

/// Compose a neutral parameter identity after admitting its complete text.
pub(crate) fn neutral_parameter_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameter: &DesignParameter,
) -> Result<cadmpeg_ir::features::ParameterId, cadmpeg_core::CodecError> {
    let stream = native_stream(&parameter.id).unwrap_or(DEFAULT_STREAM);
    let encoded_len = escaped_scope_len(ctx, stream, "retain F3D neutral parameter ID")?;
    let id = native_scoped_id_charged(
        ctx,
        "model",
        "parameter",
        EncodedParameterKey {
            stream,
            encoded_len,
            record_index: parameter.record_index,
        },
    )?;
    cadmpeg_ir::features::ParameterId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

/// The neutral parameter key from its `stream` and indexed-record identity, with
/// `stream` length-prefixed into a `#{len}:{key}` segment.
#[cfg(test)]
pub(crate) fn neutral_parameter_id_parts(
    stream: &str,
    record_index: u32,
) -> cadmpeg_ir::features::ParameterId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream);
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(record_index);
    cadmpeg_ir::features::ParameterId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "parameter"),
        key,
    )
}

/// The neutral planar-sketch key for a sketch `placement`.
#[cfg(test)]
pub(crate) fn neutral_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch"),
        sketch_placement_key(placement),
    )
}

/// The neutral spatial-sketch key for a sketch `placement`.
#[cfg(test)]
pub(crate) fn neutral_spatial_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SpatialSketchId {
    cadmpeg_ir::sketches::SpatialSketchId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch"),
        sketch_placement_key(placement),
    )
}

struct EncodedSketchKey<'a> {
    stream: &'a str,
    suffix: u64,
}

impl std::fmt::Display for EncodedSketchKey<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_escaped_identity_component(formatter, self.stream)?;
        write!(formatter, "@{}", self.suffix)
    }
}

fn neutral_sketch_key_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stream: &str,
    suffix: u64,
    kind: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    escaped_scope_len(ctx, stream, "retain F3D neutral sketch annotation ID")?;
    native_scoped_id_charged(ctx, "model", kind, EncodedSketchKey { stream, suffix })
}

/// Compose a planar sketch annotation identity under the caller's budget.
pub(crate) fn neutral_sketch_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    placement: &DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    let stream = native_stream(&placement.id).unwrap_or(DEFAULT_STREAM);
    let id = neutral_sketch_key_charged(ctx, stream, placement.entity_id.suffix(), "sketch")?;
    cadmpeg_ir::sketches::SketchId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

/// Compose a spatial sketch annotation identity under the caller's budget.
pub(crate) fn neutral_spatial_sketch_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    placement: &DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SpatialSketchId, cadmpeg_core::CodecError> {
    let stream = native_stream(&placement.id).unwrap_or(DEFAULT_STREAM);
    let id =
        neutral_sketch_key_charged(ctx, stream, placement.entity_id.suffix(), "spatial-sketch")?;
    cadmpeg_ir::sketches::SpatialSketchId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

/// The shared body of a sketch or spatial-sketch placement key: the placement's
/// stream, escaped, joined to its entity suffix by `@`.
#[cfg(test)]
fn sketch_placement_key(placement: &DesignSketchPlacement) -> cadmpeg_ir::ids::IdentityKey {
    let stream = native_stream(&placement.id).unwrap_or(DEFAULT_STREAM);
    cadmpeg_ir::identity_key!("@")
        .then(placement.entity_id.suffix())
        .with_prefix(&cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream))
}

/// The neutral planar-sketch point-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("p"),
            persistent_id,
        ),
    )
}

/// The neutral planar-sketch curve-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_curve(sketch.as_str(), primary_id, secondary_id),
    )
}

/// The neutral planar-sketch text-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_sketch_text_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("t"),
            persistent_id,
        ),
    )
}

/// The source-local neutral key for a planar sketch record that has no
/// persistent entity identity.
#[cfg(test)]
pub(crate) fn neutral_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("x"),
            u64::from(record_index),
        ),
    )
}

/// The neutral spatial-sketch curve-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_spatial_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_curve(sketch.as_str(), primary_id, secondary_id),
    )
}

/// The neutral spatial-sketch point-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_spatial_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("p"),
            persistent_id,
        ),
    )
}

/// The source-local neutral key for a spatial-sketch record that has no
/// persistent entity identity.
#[cfg(test)]
pub(crate) fn neutral_spatial_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("x"),
            u64::from(record_index),
        ),
    )
}

/// The neutral spatial-sketch surface-entity key under `sketch`.
#[cfg(test)]
pub(crate) fn neutral_spatial_sketch_surface_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("s"),
            persistent_id,
        ),
    )
}

/// A single-tag sketch-entity key: the escaped owning-sketch key, length-
/// prefixed, followed by a one-character tag (`p`/`t`/`s`/`x`) and one id.
#[cfg(test)]
fn sketch_entity_tagged(
    sketch_key: &str,
    tag: cadmpeg_ir::ids::IdentityKey,
    id: u64,
) -> cadmpeg_ir::ids::IdentityKey {
    let sketch = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(sketch_key);
    cadmpeg_ir::ids::IdentityKey::from(sketch.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&sketch)
        .then(tag)
        .then(id)
}

/// A curve sketch-entity key: the escaped owning-sketch key, length-prefixed,
/// followed by `c`, the primary id, and the colon-joined secondary id.
#[cfg(test)]
fn sketch_entity_curve(
    sketch_key: &str,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::ids::IdentityKey {
    let sketch = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(sketch_key);
    cadmpeg_ir::ids::IdentityKey::from(sketch.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&sketch)
        .then(cadmpeg_ir::identity_key!("c"))
        .then(primary_id)
        .colon(secondary_id)
}

/// The neutral sketch-constraint key for `native_ref` at `record_index`.
#[cfg(test)]
pub(crate) fn neutral_sketch_constraint_id(
    native_ref: &str,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
        native_stream(native_ref).unwrap_or(DEFAULT_STREAM),
    );
    cadmpeg_ir::sketches::SketchConstraintId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-constraint"),
        cadmpeg_ir::identity_key!("@")
            .then(record_index)
            .with_prefix(&stream),
    )
}

/// Compose a sketch constraint annotation identity under the caller's budget.
pub(crate) fn neutral_sketch_constraint_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native_ref: &str,
    record_index: u32,
) -> Result<cadmpeg_ir::sketches::SketchConstraintId, cadmpeg_core::CodecError> {
    let stream = native_stream(native_ref).unwrap_or(DEFAULT_STREAM);
    let id = neutral_sketch_key_charged(ctx, stream, u64::from(record_index), "sketch-constraint")?;
    cadmpeg_ir::sketches::SketchConstraintId::mint(id).map_err(cadmpeg_core::CodecError::malformed)
}

/// The neutral dimension-constraint key derived from a `parameter` key and a
/// dimension `form`, with the parameter key tail and form length-prefixed.
#[cfg(test)]
pub(crate) fn neutral_dimension_constraint_id(
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let parameter_key = parameter.key();
    let form = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(form);
    cadmpeg_ir::sketches::SketchConstraintId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-constraint"),
        cadmpeg_ir::identity_key!("dimension")
            .colon(parameter_key.as_str().len())
            .colon(parameter_key)
            .then(form.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&form),
    )
}

// --- history-input topology keys -------------------------------------------
//
// A history-input key names a feature's boundary topology relative to a prior
// history state. The shared body is `{len}:{feature_key}:{previous_state_id}`;
// the entity kinds (edge/face/body) append `:{slot}`, and the state key stops
// at the body.

/// The shared body of a history-input key: the feature key length-prefixed and
/// joined to `previous_state_id` by colons.
#[cfg(test)]
pub(crate) fn history_input_prefix(
    feature_key: &cadmpeg_ir::ids::IdentityKey,
    previous_state_id: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::ids::IdentityKey::from(feature_key.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .then(feature_key)
        .colon(previous_state_id)
}

/// The history-input state key for a `prefix` from [`history_input_prefix`].
#[cfg(test)]
pub(crate) fn history_input_state_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    cadmpeg_ir::ids::FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "state"),
        prefix.clone(),
    )
}

/// The history-input edge key for `slot` under a `prefix`.
#[cfg(test)]
pub(crate) fn history_input_edge_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalEdgeId {
    cadmpeg_ir::ids::HistoricalEdgeId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "edge"),
        prefix.clone().colon(slot),
    )
}

/// The history-input vertex key for `slot` under a `prefix`.
#[cfg(test)]
pub(crate) fn history_input_vertex_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalVertexId {
    cadmpeg_ir::ids::HistoricalVertexId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "vertex"),
        prefix.clone().colon(slot),
    )
}

/// The history-input face key for `slot` under a `prefix`.
#[cfg(test)]
pub(crate) fn history_input_face_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalFaceId {
    cadmpeg_ir::ids::HistoricalFaceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "face"),
        prefix.clone().colon(slot),
    )
}

/// The history-input body key for `slot` under a `prefix`.
#[cfg(test)]
pub(crate) fn history_input_body_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalBodyId {
    cadmpeg_ir::ids::HistoricalBodyId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "body"),
        prefix.clone().colon(slot),
    )
}

/// Compose one history-input entity identity without copying an intermediate key.
fn history_input_entity_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    kind: &'static str,
    slot: impl std::fmt::Display,
) -> Result<String, cadmpeg_core::CodecError> {
    let feature_key = feature
        .as_str()
        .split_once('#')
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("F3D feature identity has no key"))?
        .1;
    ctx.format_retained(
        format_args!(
            "f3d:history-input:{kind}#{}:{feature_key}:{previous_state_id}:{slot}",
            feature_key.len(),
        ),
        "retain F3D history input identity",
    )
}

macro_rules! charged_history_input_entity_id {
    ($name:ident, $type:path, $kind:literal) => {
        pub(crate) fn $name(
            ctx: &cadmpeg_core::decode::DecodeContext<'_>,
            feature: &cadmpeg_ir::features::FeatureId,
            previous_state_id: i64,
            slot: impl std::fmt::Display,
        ) -> Result<$type, cadmpeg_core::CodecError> {
            <$type>::mint(history_input_entity_id_charged(
                ctx,
                feature,
                previous_state_id,
                $kind,
                slot,
            )?)
            .map_err(cadmpeg_core::CodecError::malformed)
        }
    };
}

charged_history_input_entity_id!(
    history_input_body_id_charged,
    cadmpeg_ir::ids::HistoricalBodyId,
    "body"
);
charged_history_input_entity_id!(
    history_input_face_id_charged,
    cadmpeg_ir::ids::HistoricalFaceId,
    "face"
);
charged_history_input_entity_id!(
    history_input_edge_id_charged,
    cadmpeg_ir::ids::HistoricalEdgeId,
    "edge"
);
charged_history_input_entity_id!(
    history_input_vertex_id_charged,
    cadmpeg_ir::ids::HistoricalVertexId,
    "vertex"
);

/// Compose the history-input state identity with one retained allocation.
pub(crate) fn history_input_state_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> Result<cadmpeg_ir::ids::FeatureInputTopologyId, cadmpeg_core::CodecError> {
    let feature_key = feature
        .as_str()
        .split_once('#')
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("F3D feature identity has no key"))?
        .1;
    let value = ctx.format_retained(
        format_args!(
            "f3d:history-input:state#{}:{feature_key}:{previous_state_id}",
            feature_key.len()
        ),
        "retain F3D history input identity",
    )?;
    cadmpeg_ir::ids::FeatureInputTopologyId::mint(value)
        .map_err(cadmpeg_core::CodecError::malformed)
}

// --- native design-record keys ---------------------------------------------
//
// Native design records are keyed `f3d:{scope}:{kind}#{offset}`, where `scope`
// is the escaped archive stream name and `offset` is the record's byte offset
// or index within that stream.

/// The native scope key for an archive entry or stream `name`.
pub(crate) fn native_scope(name: &str) -> String {
    format!("f3d:{}", identity_key_component(name))
}

/// Compare an encoded identity component with source text without allocating.
pub(crate) fn encoded_identity_key_component_matches(encoded: &str, source: &str) -> bool {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut actual = encoded.as_bytes().iter();
    for character in source.chars() {
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            for &byte in bytes {
                for expected in [
                    b'%',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 0x0f)],
                ] {
                    if actual.next() != Some(&expected) {
                        return false;
                    }
                }
            }
        } else {
            for byte in bytes {
                if actual.next() != Some(byte) {
                    return false;
                }
            }
        }
    }
    actual.next().is_none()
}

/// Compare an escaped native scope without materializing the encoded entry name.
pub(crate) fn native_scope_matches(stream: &str, entry: &str) -> bool {
    fn encoded_len(source: &str) -> Option<usize> {
        source.chars().try_fold(0usize, |length, character| {
            let width = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                character.len_utf8().checked_mul(3)?
            } else {
                character.len_utf8()
            };
            length.checked_add(width)
        })
    }

    let Some(encoded_length) = encoded_len(entry) else {
        return false;
    };
    if let Some(direct) = stream.strip_prefix("f3d:") {
        if direct.len() == encoded_length && encoded_identity_key_component_matches(direct, entry) {
            return true;
        }
    }
    stream
        .strip_prefix("f3d:xref/")
        .and_then(|qualified| qualified.strip_suffix(entry))
        .is_some_and(|prefix| prefix.ends_with('/'))
}

/// Build an escaped native scope after admitting its retained byte length.
pub(crate) fn native_scope_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    let operation = "retain F3D native scope";
    let escaped_len = escaped_scope_len(ctx, scope, operation)?;
    let length = "f3d:"
        .len()
        .checked_add(escaped_len)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let mut id = ctx.retained_string(length, operation)?;
    id.push_str("f3d:");
    push_escaped_scope(&mut id, scope);
    Ok(id)
}

/// Build one record ID in an archive-entry-qualified native scope.
pub(crate) fn native_scoped_id(scope: &str, kind: &str, key: impl std::fmt::Display) -> String {
    format!("{}:{kind}#{key}", native_scope(scope))
}

/// Build a native record ID after admitting its exact retained byte length.
pub(crate) fn native_scoped_id_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &str,
    kind: &str,
    key: impl std::fmt::Display,
) -> Result<String, cadmpeg_core::CodecError> {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0 = self.0.checked_add(value.len()).ok_or(std::fmt::Error)?;
            Ok(())
        }
    }

    let operation = "retain F3D native record ID";
    let escaped_len = escaped_scope_len(ctx, scope, operation)?;
    let mut key_len = Count(0);
    std::fmt::Write::write_fmt(&mut key_len, format_args!("{key}"))
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let length = "f3d:"
        .len()
        .checked_add(escaped_len)
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(kind.len()))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(key_len.0))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let mut id = ctx.retained_string(length, operation)?;
    id.push_str("f3d:");
    push_escaped_scope(&mut id, scope);
    id.push(':');
    id.push_str(kind);
    id.push('#');
    std::fmt::Write::write_fmt(&mut id, format_args!("{key}")).map_err(|_| {
        cadmpeg_core::CodecError::malformed("F3D native record ID key formatting failed")
    })?;
    Ok(id)
}

fn escaped_scope_len(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &str,
    operation: &'static str,
) -> Result<usize, cadmpeg_core::CodecError> {
    let scope_len =
        u64::try_from(scope.len()).map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_work(
        scope_len
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?,
        "escape F3D native scope",
    )?;
    let mut escaped_len = 0usize;
    for character in scope.chars() {
        let width = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            character.len_utf8().checked_mul(3)
        } else {
            Some(character.len_utf8())
        }
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
        escaped_len = escaped_len
            .checked_add(width)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    }
    Ok(escaped_len)
}

fn push_escaped_scope(id: &mut String, scope: &str) {
    for character in scope.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                id.push('%');
                id.push(char::from(HEX[usize::from(byte >> 4)]));
                id.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            id.push(character);
        }
    }
}

/// Macro defining one `f3d:{scope}:{kind}#{offset}` native-record builder.
macro_rules! native_record_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(scope: &str, offset: impl std::fmt::Display) -> String {
            native_scoped_id(scope, $kind, offset)
        }
    };
}

native_record_id!(
    /// The native design-parameter record key.
    native_design_parameter_id,
    "design-parameter"
);
native_record_id!(
    /// The native design-parameter-owner record key.
    native_design_parameter_owner_id,
    "design-parameter-owner"
);
native_record_id!(
    /// The native ordered Design feature-timeline record key.
    #[cfg(test)]
    native_design_feature_timeline_id,
    "design-feature-timeline"
);
native_record_id!(
    /// The native Design component naming-space binding key.
    #[cfg(test)]
    native_design_component_naming_space_id,
    "design-component-naming-space"
);

/// The native ordered Design feature-timeline key in an already encoded
/// `f3d:` stream scope.
#[cfg(test)]
pub(crate) fn native_design_feature_timeline_id_in_stream(
    stream: &str,
    offset: impl std::fmt::Display,
) -> String {
    format!("{stream}:design-feature-timeline#{offset}")
}
native_record_id!(
    /// The native design Canvas image-plane binding key.
    native_design_canvas_image_id,
    "design-canvas-image"
);
native_record_id!(
    /// The native design Decal image and target binding key.
    native_design_decal_image_id,
    "design-decal-image"
);
native_record_id!(
    /// The native persistent-reference record key.
    #[cfg(test)]
    native_persistent_reference_id,
    "persistent-reference"
);
native_record_id!(
    /// The native lost-edge-reference record key.
    #[cfg(test)]
    native_lost_edge_reference_id,
    "lost-edge-reference"
);
native_record_id!(
    /// The native design-type record key.
    #[cfg(test)]
    native_design_type_id,
    "design-type"
);
native_record_id!(
    /// The native design-record-header record key.
    #[cfg(test)]
    native_design_record_header_id,
    "design-record-header"
);
native_record_id!(
    /// The native sketch-text record key.
    #[cfg(test)]
    native_sketch_text_id,
    "sketch-text"
);
native_record_id!(
    /// The native sketch-surface record key.
    native_sketch_surface_id,
    "sketch-surface"
);
native_record_id!(
    /// The native mesh-body record key.
    #[cfg(test)]
    native_mesh_body_id,
    "mesh-body"
);
native_record_id!(
    /// The native Design mesh-feature graph key.
    #[cfg(test)]
    native_design_mesh_feature_id,
    "design-mesh-feature"
);
native_record_id!(
    /// The native design-body-member record key.
    #[cfg(test)]
    native_design_body_member_id,
    "design-body-member"
);
native_record_id!(
    /// The native design-body-binding record key.
    native_design_body_binding_id,
    "design-body-binding"
);

#[cfg(test)]
mod tests {
    use super::{
        decode_identity_key_component, design_segment, face_appearance_binding_id,
        native_design_feature_timeline_id_in_stream, native_design_type_id, native_scope,
        neutral_assembly_legacy_object_id, neutral_sketch_record_id, neutral_sketch_text_id,
        same_native_occurrence, SCHEME_PREFIX,
    };
    use crate::records::{
        feature::assembly::DesignAssemblyLegacySelection, recipes::ConstructionRecipeKind,
    };

    #[test]
    fn charged_native_id_matches_escaped_identity_bytes() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scope = "A B:#%\u{2003}é";
        let actual = super::native_scoped_id_charged(&ctx, scope, "act-guid", 42).unwrap();
        assert_eq!(actual, super::native_scoped_id(scope, "act-guid", 42));
    }

    #[test]
    fn charged_native_scope_matches_escaped_identity_bytes() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scope = "A B:#%\u{2003}é";
        assert_eq!(
            super::native_scope_charged(&ctx, scope).unwrap(),
            native_scope(scope)
        );
    }

    #[test]
    fn native_scope_match_preserves_direct_and_xref_comparisons() {
        for entry in ["Design/BulkStream.dat", "A B:#%\u{2003}é", ""] {
            let direct = super::native_scope(entry);
            assert!(super::native_scope_matches(&direct, entry));
            assert!(!super::native_scope_matches(&direct, "different"));
            let xref = format!("f3d:xref/part/{entry}");
            assert!(super::native_scope_matches(&xref, entry));
            assert!(!super::native_scope_matches(&format!("{xref}extra"), entry));
        }
    }

    #[test]
    fn charged_native_scope_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::native_scope_charged(&ctx, "Design/BulkStream.dat").unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D native scope")
        );
    }

    #[test]
    fn charged_native_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            super::native_scoped_id_charged(&ctx, "ACT/BulkStream.dat", "act-guid", 1).unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "retain F3D native record ID"
        ));
    }

    #[test]
    fn neutral_assembly_joint_id_charged_matches_context_free_bytes() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#7",
            crate::records::feature::scope::DesignFeatureKind::Assemble,
            7,
        );
        assert_eq!(
            super::neutral_assembly_joint_id(&ctx, &scope).unwrap(),
            crate::test_support::with_decode_context(|ctx| super::neutral_assembly_joint_id(
                ctx, &scope
            ))
            .unwrap(),
        );
    }

    #[test]
    fn neutral_assembly_joint_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#7",
            crate::records::feature::scope::DesignFeatureKind::Assemble,
            7,
        );
        let error = super::neutral_assembly_joint_id(&ctx, &scope).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D native record ID")
        );
    }

    #[test]
    fn history_keys_preserve_admitted_colons_percent_escapes_and_signed_states() {
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#a:b%20c").unwrap();
        let prefix = super::history_input_prefix(&feature.key(), -3);
        assert_eq!(prefix.as_str(), "7:a:b%20c:-3");
        assert_eq!(
            super::history_input_edge_id(&prefix, 9).as_str(),
            "f3d:history-input:edge#7:a:b%20c:-3:9"
        );
    }

    #[test]
    fn charged_history_input_ids_match_existing_identity_bytes() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#a:b%20c").unwrap();
        let prefix = super::history_input_prefix(&feature.key(), -3);
        assert_eq!(
            super::history_input_body_id_charged(&ctx, &feature, -3, 9).unwrap(),
            super::history_input_body_id(&prefix, 9)
        );
        assert_eq!(
            super::history_input_face_id_charged(&ctx, &feature, -3, 9).unwrap(),
            super::history_input_face_id(&prefix, 9)
        );
        assert_eq!(
            super::history_input_edge_id_charged(&ctx, &feature, -3, 9).unwrap(),
            super::history_input_edge_id(&prefix, 9)
        );
        assert_eq!(
            super::history_input_vertex_id_charged(&ctx, &feature, -3, 9).unwrap(),
            super::history_input_vertex_id(&prefix, 9)
        );
        assert_eq!(
            super::history_input_state_id_charged(&ctx, &feature, -3).unwrap(),
            super::history_input_state_id(&prefix)
        );
    }

    #[test]
    fn charged_history_input_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#a:b%20c").unwrap();
        let error = super::history_input_edge_id_charged(&ctx, &feature, -3, 9).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D history input identity")
        );
    }

    #[test]
    fn empty_stream_remains_empty_before_the_sketch_entity_suffix() {
        let placement = crate::records::sketch_placement::DesignSketchPlacement {
            id: ":record#4".into(),
            scope_record_index: None,
            entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 7),
            visibility: None,
            class_tag: "330".to_owned().try_into().unwrap(),
            record_index: 4,
            paired_class_tag: "330".to_owned().try_into().unwrap(),
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                0,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
            )
            .unwrap(),
        };
        assert_eq!(
            super::neutral_sketch_id(&placement).as_str(),
            "f3d:model:sketch#@7"
        );
        assert_eq!(
            super::neutral_sketch_constraint_id(&placement.id, 4).as_str(),
            "f3d:model:sketch-constraint#@4"
        );
    }

    #[test]
    fn admitted_guid_and_visual_tokens_retain_source_spelling() {
        let text = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        let guid: crate::records::mesh::DesignRelaxedGuidText = text.to_owned().try_into().unwrap();
        assert_eq!(
            super::neutral_component_id(&guid).as_str(),
            "f3d:model:component#aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
        );
        assert_eq!(
            serde_json::to_string(&guid).unwrap(),
            serde_json::to_string(text).unwrap()
        );
        let token_text = format!("{text}_Post2015_Post2015");
        let token: crate::records::references::DesignVisualToken =
            token_text.clone().try_into().unwrap();
        assert_eq!(
            super::appearance_id(token.identity_key()).as_str(),
            format!("f3d:design:appearance#{token_text}")
        );
        assert_eq!(
            serde_json::to_string(&token).unwrap(),
            serde_json::to_string(&token_text).unwrap()
        );
        assert!(crate::records::mesh::DesignRelaxedGuidText::try_from(String::new()).is_err());
        assert!(crate::records::references::DesignVisualToken::try_from(String::new()).is_err());
    }

    #[test]
    fn raw_appearance_components_keep_legal_separators_and_reject_whitespace() {
        let id = super::assignment_appearance_binding_id(
            "part:one%20_7",
            cadmpeg_ir::identity_key!("visual"),
        )
        .unwrap();
        assert_eq!(id.as_str(), "f3d:appearance:binding#part:one%20_7:visual");
        assert!(super::assignment_appearance_binding_id(
            "part one_7",
            cadmpeg_ir::identity_key!("visual")
        )
        .is_err());
        assert_eq!(
            super::subd_id("cage:one%20").unwrap().as_str(),
            "f3d:tspline:subd#cage:one%20"
        );
    }

    #[test]
    fn charged_appearance_ids_preserve_identity_text() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let visual = cadmpeg_ir::ids::IdentityKey::try_new("visual").unwrap();
        assert_eq!(
            super::appearance_id_charged(&ctx, visual.as_str()).unwrap(),
            super::appearance_id(visual.clone())
        );
        assert_eq!(
            super::body_appearance_binding_id_charged(&ctx, 42, visual.as_str()).unwrap(),
            super::body_appearance_binding_id(42, visual.clone())
        );
        assert_eq!(
            super::assignment_appearance_binding_id_charged(
                &ctx,
                "part:one%20_7",
                visual.as_str(),
            )
            .unwrap(),
            super::assignment_appearance_binding_id("part:one%20_7", visual).unwrap()
        );
    }

    #[test]
    fn appearance_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::appearance_id_charged(&ctx, "visual").unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D native record ID")
        );
    }

    #[test]
    fn body_appearance_binding_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::body_appearance_binding_id_charged(&ctx, 42, "visual").unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D native record ID")
        );
    }

    #[test]
    fn assignment_appearance_binding_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            super::assignment_appearance_binding_id_charged(&ctx, "part:one%20_7", "visual")
                .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D native record ID")
        );
    }

    #[test]
    fn design_segment_joins_sibling_meta_and_bulk_stream_ids() {
        let meta = "f3d:Asset/Design1/MetaStream.dat:design-type#10";
        let bulk = "f3d:Asset/Design1/BulkStream.dat:design-canvas-image#20";
        assert_eq!(design_segment(meta), Some("f3d:Asset/Design1"));
        assert_eq!(design_segment(meta), design_segment(bulk));
        assert_eq!(
            design_segment("f3d:Asset/Design1/Other.dat:record#20"),
            None
        );
    }

    #[test]
    fn native_ids_escape_archive_names_without_losing_the_raw_stream() {
        let entry = "Simulation Case/Design:1/MetaStream%20.dat";
        let id = native_design_type_id(entry, 10);
        assert_eq!(
            id,
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-type#10"
        );
        let encoded = native_scope(entry)
            .strip_prefix(SCHEME_PREFIX)
            .expect("native scheme")
            .to_owned();
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| decode_identity_key_component(
                ctx, &encoded
            )
            .expect("service admission"))
            .as_deref(),
            Some(entry)
        );
        assert_eq!(
            crate::writer::patch::records::native_stream(&id, ":design-type#")
                .expect("writer stream"),
            entry
        );
        assert_eq!(
            native_design_feature_timeline_id_in_stream(&native_scope(entry), 20),
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-feature-timeline#20"
        );
    }

    #[test]
    fn face_appearance_binding_escapes_the_nested_face_identity() {
        let id = face_appearance_binding_id(
            "face-guid",
            cadmpeg_ir::identity_key!("visual-guid"),
            &cadmpeg_ir::ids::FaceId::mint("f3d:brep/path:face#12").expect("identity grammar"),
        )
        .expect("valid test identity");
        assert_eq!(
            id.as_str(),
            "f3d:appearance:face#face-guid:visual-guid:27:f3d%3Abrep/path%3Aface%2312"
        );
        assert_eq!(id.as_str().matches('#').count(), 1);
    }

    #[test]
    fn native_occurrence_scope_isolates_xrefs_and_includes_root_streams() {
        assert!(same_native_occurrence(
            "f3d:Asset/Design1/BulkStream.dat:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
        assert!(same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/xref/child/occurrence-0/design:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-invalid/design:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
    }

    #[test]
    fn identityless_sketch_geometry_uses_a_disjoint_source_record_namespace() {
        let sketch = cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#example").unwrap();
        let persistent = neutral_sketch_text_id(&sketch, 42);
        let source_record = neutral_sketch_record_id(&sketch, 42);
        assert_ne!(persistent, source_record);
        assert_eq!(source_record, neutral_sketch_record_id(&sketch, 42));
        assert_ne!(source_record, neutral_sketch_record_id(&sketch, 43));
    }

    #[test]
    fn legacy_assembly_connector_key_is_namespace_and_recipe_scoped() {
        let selection = DesignAssemblyLegacySelection {
            record_index: 7,
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 110,
            context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 120,
            recipe_record_index: 8,
            recipe_record_byte_offset: 130,
            recipe_id: "Recipe:1".into(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_references: Vec::new(),
            next_byte_offset: 140,
        };
        assert_eq!(
            neutral_assembly_legacy_object_id(&selection),
            "f3d:feature-input:connector#assembly-legacy:36:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:36:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:10:recipe%3A1:7:8"
        );
        let mut second = selection.clone();
        second.record_index += 1;
        assert_ne!(
            neutral_assembly_legacy_object_id(&selection),
            neutral_assembly_legacy_object_id(&second)
        );
    }
}
