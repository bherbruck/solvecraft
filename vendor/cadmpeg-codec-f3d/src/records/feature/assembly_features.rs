// SPDX-License-Identifier: Apache-2.0
//! Component insert, derived instance, occurrence and copy-paste component features.

use crate::records::identity::{Located, IDENTITY_MATRIX};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(
    deserialize_carrier_transform_offset,
    u64,
    "carrier_transform_offset"
);
cadmpeg_core::named_optional_field!(deserialize_occurrence_identity, u64, "occurrence_identity");
cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignComponentInsertConstructionWire")]
pub(crate) struct DesignComponentInsertConstruction {
    /// Scope-owned relation record.
    pub(crate) relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    pub(crate) carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    pub(crate) neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    pub(crate) neutron_role_offset: u64,
    /// Explicit scope-local placement and its optional repeated carrier location.
    /// Absence is the encoded identity form.
    pub(crate) placement: Option<DesignComponentInsertMatrix>,
}

#[cfg(test)]
thread_local! {
    static COMPONENT_INSERT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignComponentInsertConstruction {
    fn clone(&self) -> Self {
        COMPONENT_INSERT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            relation_record_index: self.relation_record_index,
            carrier_record_index: self.carrier_record_index,
            occurrence_identity: self.occurrence_identity,
            neutron_role: self.neutron_role.clone(),
            neutron_role_offset: self.neutron_role_offset,
            placement: self.placement.clone(),
        }
    }
}

/// Scope-local matrix with an optional equal matrix in the grouped carrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignComponentInsertMatrix {
    pub(crate) scope: Located<SketchPlacementMatrix>,
    pub(crate) carrier_offset: Option<u64>,
}

impl DesignComponentInsertConstruction {
    #[must_use]
    pub(crate) fn transform(&self) -> &SketchPlacementMatrix {
        self.placement
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| {
                &matrix.scope.value
            })
    }

    #[must_use]
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.placement.as_ref().map(|matrix| matrix.scope.offset)
    }

    #[must_use]
    pub(crate) fn carrier_transform_offset(&self) -> Option<u64> {
        self.placement
            .as_ref()
            .and_then(|matrix| matrix.carrier_offset)
    }
}

/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentInsertConstructionWire {
    /// Scope-owned relation record.
    relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_identity"
    )]
    occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    neutron_role_offset: u64,
    /// Row-major local occurrence transform in centimetres.
    transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar. `None` is the
    /// stored identity form, which has no scalar block.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Byte offset of the equal transform's first scalar in the grouped
    /// carrier; absent when the carrier stores no scalar block.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier_transform_offset"
    )]
    carrier_transform_offset: Option<u64>,
}

impl Serialize for DesignComponentInsertConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            relation_record_index: u32,
            carrier_record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            occurrence_identity: Option<u64>,
            neutron_role: &'a str,
            neutron_role_offset: u64,
            transform: SketchPlacementMatrix,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            carrier_transform_offset: Option<u64>,
        }
        WireRef {
            relation_record_index: self.relation_record_index,
            carrier_record_index: self.carrier_record_index,
            occurrence_identity: self.occurrence_identity,
            neutron_role: &self.neutron_role,
            neutron_role_offset: self.neutron_role_offset,
            transform: *self.transform(),
            transform_offset: self.transform_offset(),
            carrier_transform_offset: self.carrier_transform_offset(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignComponentInsertConstructionWire> for DesignComponentInsertConstruction {
    type Error = String;
    fn try_from(wire: DesignComponentInsertConstructionWire) -> Result<Self, Self::Error> {
        let placement = match (wire.transform_offset, wire.carrier_transform_offset) {
            (Some(offset), carrier_offset) => Some(DesignComponentInsertMatrix {
                scope: Located {
                    value: wire.transform,
                    offset,
                },
                carrier_offset,
            }),
            (None, None) => {
                if wire
                    .transform
                    .iter()
                    .flatten()
                    .zip(IDENTITY_MATRIX.iter().flatten())
                    .any(|(value, identity)| value.to_bits() != identity.to_bits())
                {
                    return Err("transform must be identity when transform_offset is absent".into());
                }
                None
            }
            (None, Some(_)) => {
                return Err("carrier_transform_offset requires transform_offset".into())
            }
        };
        Ok(Self {
            relation_record_index: wire.relation_record_index,
            carrier_record_index: wire.carrier_record_index,
            occurrence_identity: wire.occurrence_identity,
            neutron_role: wire.neutron_role,
            neutron_role_offset: wire.neutron_role_offset,
            placement,
        })
    }
}

#[cfg(test)]
impl From<DesignComponentInsertConstruction> for DesignComponentInsertConstructionWire {
    fn from(record: DesignComponentInsertConstruction) -> Self {
        let transform = *record.transform();
        let transform_offset = record.transform_offset();
        let carrier_transform_offset = record.carrier_transform_offset();
        Self {
            relation_record_index: record.relation_record_index,
            carrier_record_index: record.carrier_record_index,
            occurrence_identity: record.occurrence_identity,
            neutron_role: record.neutron_role,
            neutron_role_offset: record.neutron_role_offset,
            transform,
            transform_offset,
            carrier_transform_offset,
        }
    }
}

/// Local component occurrence joined through a `DerivedInstance` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDerivedInstanceConstruction {
    /// Scope prologue record referenced by the fixed field at scope offset 22.
    pub(crate) reference_record_index: u32,
    /// Scope-owned class-310 relation record.
    pub(crate) relation_record_index: u32,
    /// Class-380 component-occurrence carrier named by the relation.
    pub(crate) carrier_record_index: u32,
    /// Component definition GUID carried by the joined occurrence.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Placed occurrence GUID carried by the joined occurrence.
    pub(crate) occurrence_guid: DesignRelaxedGuidText,
    /// Row-major local-to-model placement in centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar.
    pub(crate) transform_offset: u64,
}

/// One exact local component-occurrence carrier.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignComponentOccurrenceWire")]
pub(crate) struct DesignComponentOccurrence {
    /// Stable native record identity.
    pub(crate) id: String,
    /// Indexed-record class carrying this occurrence.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    component_record_index: u64,
    /// Stable component-definition GUID.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence GUID.
    pub(crate) occurrence_guid: DesignRelaxedGuidText,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    placement: DesignComponentOccurrencePlacement,
}

#[cfg(test)]
thread_local! {
    static COMPONENT_OCCURRENCE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignComponentOccurrence {
    fn clone(&self) -> Self {
        COMPONENT_OCCURRENCE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            component_record_index: self.component_record_index,
            component_guid: self.component_guid.clone(),
            occurrence_guid: self.occurrence_guid.clone(),
            placement: self.placement,
        }
    }
}

/// Local occurrence payload before checked frame admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignComponentOccurrenceDraft {
    /// Stable native record identity.
    pub(crate) id: String,
    /// Indexed-record class carrying this occurrence.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed header.
    pub(crate) byte_offset: u64,
    /// Referenced component-definition record.
    pub(crate) component_record_index: u64,
    /// Stable component-definition GUID.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence GUID.
    pub(crate) occurrence_guid: DesignRelaxedGuidText,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    pub(crate) placement: DesignComponentOccurrencePlacement,
}

/// Placement envelope of a local component occurrence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DesignComponentOccurrencePlacement {
    /// First occurrence, with no explicit matrix payload.
    Base,
    /// Explicit matrix and one-based occurrence ordinal.
    Explicit {
        ordinal: NonZeroU32,
        transform: SketchPlacementMatrix,
    },
}

impl DesignComponentOccurrence {
    /// Admit a local occurrence with representable GUID and placement offsets.
    pub(crate) fn try_new(draft: DesignComponentOccurrenceDraft) -> Result<Self, String> {
        let last_offset = match draft.placement {
            DesignComponentOccurrencePlacement::Base => 124,
            DesignComponentOccurrencePlacement::Explicit { .. } => 209,
        };
        draft
            .byte_offset
            .checked_add(last_offset)
            .ok_or("component occurrence offsets overflow byte_offset")?;
        Ok(Self {
            id: draft.id,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            component_record_index: draft.component_record_index,
            component_guid: draft.component_guid,
            occurrence_guid: draft.occurrence_guid,
            placement: draft.placement,
        })
    }

    /// Indexed header byte offset.
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Component GUID byte offset.
    fn component_guid_offset(&self) -> u64 {
        self.byte_offset + 48
    }

    /// Occurrence GUID byte offset.
    fn occurrence_guid_offset(&self) -> u64 {
        self.byte_offset + 124
    }

    /// Base or explicit local placement.
    pub(crate) fn placement(&self) -> &DesignComponentOccurrencePlacement {
        &self.placement
    }

    #[must_use]
    pub(crate) fn occurrence_ordinal(&self) -> u32 {
        match self.placement {
            DesignComponentOccurrencePlacement::Base => 1,
            DesignComponentOccurrencePlacement::Explicit { ordinal, .. } => ordinal.get(),
        }
    }

    #[must_use]
    pub(crate) fn transform(&self) -> Option<Located<SketchPlacementMatrix>> {
        match self.placement {
            DesignComponentOccurrencePlacement::Base => None,
            DesignComponentOccurrencePlacement::Explicit { transform, .. } => Some(Located {
                value: transform,
                offset: self.byte_offset + 209,
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DesignComponentOccurrenceWire {
    /// Stable native record identity.
    id: String,
    /// Indexed-record class carrying this occurrence.
    class_tag: String,
    /// Indexed carrier record.
    record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    component_record_index: u64,
    /// Stable component-definition GUID.
    component_guid: DesignRelaxedGuidText,
    /// Byte offset of the component GUID payload.
    component_guid_offset: u64,
    /// Stable placed-occurrence GUID.
    occurrence_guid: DesignRelaxedGuidText,
    /// Byte offset of the occurrence GUID payload.
    occurrence_guid_offset: u64,
    /// One-based occurrence ordinal within the component definition.
    occurrence_ordinal: u32,
    /// Explicit local-to-model placement for placed occurrences.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the explicit placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
}

impl Serialize for DesignComponentOccurrence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            class_tag: &'a str,
            record_index: u32,
            byte_offset: u64,
            component_record_index: u64,
            component_guid: &'a DesignRelaxedGuidText,
            component_guid_offset: u64,
            occurrence_guid: &'a DesignRelaxedGuidText,
            occurrence_guid_offset: u64,
            occurrence_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform: Option<SketchPlacementMatrix>,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
        }
        let transform = self.transform();
        WireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            component_record_index: self.component_record_index,
            component_guid: &self.component_guid,
            component_guid_offset: self.component_guid_offset(),
            occurrence_guid: &self.occurrence_guid,
            occurrence_guid_offset: self.occurrence_guid_offset(),
            occurrence_ordinal: self.occurrence_ordinal(),
            transform: transform.map(|frame| frame.value),
            transform_offset: transform.map(|frame| frame.offset),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<DesignComponentOccurrence> for DesignComponentOccurrenceWire {
    fn from(value: DesignComponentOccurrence) -> Self {
        let occurrence_ordinal = value.occurrence_ordinal();
        let component_guid_offset = value.component_guid_offset();
        let occurrence_guid_offset = value.occurrence_guid_offset();
        let transform = value.transform();
        Self {
            id: value.id,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            component_guid_offset,
            occurrence_guid: value.occurrence_guid,
            occurrence_guid_offset,
            occurrence_ordinal,
            transform: transform.map(|frame| frame.value),
            transform_offset: transform.map(|frame| frame.offset),
        }
    }
}

impl TryFrom<DesignComponentOccurrenceWire> for DesignComponentOccurrence {
    type Error = String;
    fn try_from(value: DesignComponentOccurrenceWire) -> Result<Self, Self::Error> {
        let transform = Located::from_wire(value.transform, value.transform_offset, "transform")?;
        let placement = match (value.occurrence_ordinal, transform) {
            (1, None) => DesignComponentOccurrencePlacement::Base,
            (ordinal, Some(transform)) => DesignComponentOccurrencePlacement::Explicit {
                ordinal: NonZeroU32::new(ordinal).ok_or("occurrence_ordinal must be nonzero")?,
                transform: transform.value,
            },
            (_, None) => return Err("occurrence_ordinal must be 1 when transform is absent".into()),
        };
        let record = Self::try_new(DesignComponentOccurrenceDraft {
            id: value.id,
            class_tag: value.class_tag.try_into()?,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            occurrence_guid: value.occurrence_guid,
            placement,
        })?;
        if value.component_guid_offset != record.component_guid_offset() {
            return Err("component_guid_offset disagrees with byte_offset".into());
        }
        if value.occurrence_guid_offset != record.occurrence_guid_offset() {
            return Err("occurrence_guid_offset disagrees with byte_offset".into());
        }
        if value.transform_offset != record.transform().map(|transform| transform.offset) {
            return Err("transform_offset disagrees with byte_offset".into());
        }
        Ok(record)
    }
}

/// Legacy component copy/paste construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCopyPasteComponentOperation {
    /// Scope-owned relation record.
    pub(crate) relation_record_index: u32,
    /// Existing source occurrence carrier.
    pub(crate) source_occurrence_record_index: u32,
    /// Newly copied occurrence carrier.
    pub(crate) copied_occurrence_record_index: u32,
    /// Reusable component definition shared by source and copy.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Existing source occurrence identity.
    pub(crate) source_occurrence_guid: DesignRelaxedGuidText,
    /// Newly copied occurrence identity.
    pub(crate) copied_occurrence_guid: DesignRelaxedGuidText,
    /// Source placement embedded by the scope.
    pub(crate) source_transform: SketchPlacementMatrix,
    /// Byte offset of the source placement.
    pub(crate) source_transform_offset: u64,
    /// Copied placement embedded by both scope and occurrence carrier.
    pub(crate) copied_transform: SketchPlacementMatrix,
    /// Byte offset of the scope-local copied placement.
    pub(crate) copied_transform_offset: u64,
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
