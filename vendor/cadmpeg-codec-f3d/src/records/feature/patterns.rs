// SPDX-License-Identifier: Apache-2.0
//! Circular and rectangular pattern constructions and their instances.

use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(
    deserialize_component_occurrences,
    DesignComponentPatternOccurrencesWire,
    "component_occurrences"
);
cadmpeg_core::named_optional_field!(
    deserialize_instances,
    DesignRectangularPatternInstances,
    "instances"
);
/// Exact construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCircularPatternConstruction {
    /// Positive total instance count, including the seed.
    pub(crate) count: NonZeroU32,
    /// Referenced compact count-parameter owner.
    pub(crate) count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub(crate) count_offset: u64,
    /// Angular span.
    pub(crate) angle: cadmpeg_ir::scalar::PositiveAngle,
    /// Referenced total-angle scalar.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the total-angle scalar.
    pub(crate) angle_offset: u64,
    /// Serialized axis construction and its resolved placement.
    pub(crate) axis: DesignCircularPatternAxis,
    /// Referenced axis record.
    pub(crate) axis_record_index: u32,
    /// Referenced persistent selection operand.
    pub(crate) selection_record_index: u32,
}

/// Proven origin and unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignAxis {
    pub(crate) origin: FinitePoint3,
    pub(crate) direction: UnitVector3,
}

impl DesignAxis {
    pub(crate) fn from_parts(origin: FinitePoint3, direction: FiniteVector3) -> Option<Self> {
        Some(Self {
            origin,
            direction: UnitVector3::normalized(direction.get())?,
        })
    }
}

/// Proven origin and unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignPlane {
    origin: FinitePoint3,
    normal: UnitVector3,
}

impl DesignPlane {
    pub(crate) fn from_parts(origin: FinitePoint3, normal: FiniteVector3) -> Option<Self> {
        Some(Self {
            origin,
            normal: UnitVector3::new(normal.get())?,
        })
    }
    pub(crate) fn origin(self) -> FinitePoint3 {
        self.origin
    }
    pub(crate) fn normal(self) -> UnitVector3 {
        self.normal
    }
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCircularPatternAxisWire")]
pub(crate) enum DesignCircularPatternAxis {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [FiniteReal; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Axis direction; the decoder stores the serialized displacement at unit
        /// length.
        direction: UnitVector3,
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced wrappers and the offsets of their shared identity.
        wrappers: cadmpeg_ir::features::NonEmptyMembers<DesignPatternAxisWrapper>,
        /// Persistent ASM identity shared by the wrappers.
        persistent_identity: u64,
        /// Resolved model-space axis, when exact.
        resolved: Option<DesignAxis>,
    },
}

#[cfg(test)]
thread_local! {
    static CIRCULAR_PATTERN_AXIS_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCircularPatternAxis {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CIRCULAR_PATTERN_AXIS_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        match self {
            Self::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::Inline {
                origin: *origin,
                origin_offset: *origin_offset,
                direction: *direction,
                direction_offset: *direction_offset,
            },
            Self::HistoricalEdge {
                wrappers,
                persistent_identity,
                resolved,
            } => Self::HistoricalEdge {
                wrappers: wrappers.clone(),
                persistent_identity: *persistent_identity,
                resolved: *resolved,
            },
        }
    }
}

struct PatternAxisWrapperIndices<'a>(&'a [DesignPatternAxisWrapper]);
struct PatternAxisIdentityOffsets<'a>(&'a [DesignPatternAxisWrapper]);

impl Serialize for PatternAxisWrapperIndices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|wrapper| wrapper.record_index))
    }
}

impl Serialize for PatternAxisIdentityOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|wrapper| wrapper.identity_offset))
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignCircularPatternAxisRef<'a> {
    Inline {
        origin: [FiniteReal; 3],
        origin_offset: u64,
        direction: [f64; 3],
        direction_offset: u64,
    },
    HistoricalEdge {
        wrapper_record_indices: PatternAxisWrapperIndices<'a>,
        persistent_identities: [u64; 1],
        identity_offsets: PatternAxisIdentityOffsets<'a>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resolved_origin: Option<FinitePoint3>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resolved_direction: Option<FiniteVector3>,
    },
}

impl Serialize for DesignCircularPatternAxis {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let borrowed = match self {
            Self::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => DesignCircularPatternAxisRef::Inline {
                origin: *origin,
                origin_offset: *origin_offset,
                direction: [
                    direction.as_raw().x,
                    direction.as_raw().y,
                    direction.as_raw().z,
                ],
                direction_offset: *direction_offset,
            },
            Self::HistoricalEdge {
                wrappers,
                persistent_identity,
                resolved,
            } => DesignCircularPatternAxisRef::HistoricalEdge {
                wrapper_record_indices: PatternAxisWrapperIndices(wrappers),
                persistent_identities: [*persistent_identity],
                identity_offsets: PatternAxisIdentityOffsets(wrappers),
                resolved_origin: resolved.map(|axis| axis.origin),
                resolved_direction: resolved.map(|axis| FiniteVector3::from(axis.direction)),
            },
        };
        borrowed.serialize(serializer)
    }
}

/// One historical axis wrapper and the location of its persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignPatternAxisWrapper {
    pub(crate) record_index: u32,
    pub(crate) identity_offset: u64,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignCircularPatternAxisWire {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [FiniteReal; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Axis direction; the decoder stores the serialized displacement at unit
        /// length.
        direction: [f64; 3],
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced Design wrapper records, in serialized order.
        wrapper_record_indices: Vec<u32>,
        /// Persistent ASM identities carried by the wrappers.
        persistent_identities: Vec<u64>,
        /// Identity byte offsets parallel to `wrapper_record_indices`.
        identity_offsets: Vec<u64>,
        /// Resolved model-space axis, when exact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_origin: Option<FinitePoint3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_direction: Option<FiniteVector3>,
    },
}

const EPS_CIRCULAR_PATTERN_AXIS_UNIT: f64 = 1.0e-12;

impl DesignCircularPatternAxis {
    pub(crate) fn inline(
        origin: [FiniteReal; 3],
        origin_offset: u64,
        displacement: [f64; 3],
        direction_offset: u64,
    ) -> Option<Self> {
        let raw = Vector3::from(displacement);
        let length = raw.x.hypot(raw.y).hypot(raw.z);
        if !length.is_finite() || length <= f64::EPSILON {
            return None;
        }
        let direction = if (length - 1.0).abs() <= EPS_CIRCULAR_PATTERN_AXIS_UNIT {
            UnitVector3::new(raw)?
        } else {
            UnitVector3::normalized(raw)?
        };
        Some(Self::Inline {
            origin,
            origin_offset,
            direction,
            direction_offset,
        })
    }
}

impl TryFrom<DesignCircularPatternAxisWire> for DesignCircularPatternAxis {
    type Error = String;

    fn try_from(wire: DesignCircularPatternAxisWire) -> Result<Self, Self::Error> {
        match wire {
            DesignCircularPatternAxisWire::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::inline(origin, origin_offset, direction, direction_offset)
                .ok_or_else(|| "inline axis direction must be nonzero and finite".into()),
            DesignCircularPatternAxisWire::HistoricalEdge {
                wrapper_record_indices,
                persistent_identities,
                identity_offsets,
                resolved_origin,
                resolved_direction,
            } => {
                if wrapper_record_indices.len() != identity_offsets.len() {
                    return Err(
                        "wrapper_record_indices and identity_offsets must have equal lengths"
                            .into(),
                    );
                }
                let [persistent_identity] = persistent_identities.as_slice() else {
                    return Err("persistent_identities must contain one shared identity".into());
                };
                let resolved = match (resolved_origin, resolved_direction) {
                    (None, None) => None,
                    (Some(origin), Some(direction)) => Some(
                        DesignAxis::from_parts(origin, direction)
                            .ok_or("resolved axis direction must be nonzero")?,
                    ),
                    _ => {
                        return Err(
                            "resolved_origin and resolved_direction must occur together".into()
                        )
                    }
                };
                Ok(Self::HistoricalEdge {
                    wrappers: wrapper_record_indices
                        .into_iter()
                        .zip(identity_offsets)
                        .map(|(record_index, identity_offset)| DesignPatternAxisWrapper {
                            record_index,
                            identity_offset,
                        })
                        .collect::<Vec<_>>()
                        .try_into()
                        .map_err(|_| "historical axis wrappers must be nonempty")?,
                    persistent_identity: *persistent_identity,
                    resolved,
                })
            }
        }
    }
}

#[cfg(test)]
impl From<DesignCircularPatternAxis> for DesignCircularPatternAxisWire {
    fn from(axis: DesignCircularPatternAxis) -> Self {
        match axis {
            DesignCircularPatternAxis::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::Inline {
                origin,
                origin_offset,
                direction: [
                    direction.as_raw().x,
                    direction.as_raw().y,
                    direction.as_raw().z,
                ],
                direction_offset,
            },
            DesignCircularPatternAxis::HistoricalEdge {
                wrappers,
                persistent_identity,
                resolved,
            } => Self::HistoricalEdge {
                wrapper_record_indices: wrappers
                    .iter()
                    .map(|wrapper| wrapper.record_index)
                    .collect(),
                persistent_identities: vec![persistent_identity],
                identity_offsets: wrappers
                    .iter()
                    .map(|wrapper| wrapper.identity_offset)
                    .collect(),
                resolved_origin: resolved.map(|axis| axis.origin),
                resolved_direction: resolved.map(|axis| FiniteVector3::from(axis.direction)),
            },
        }
    }
}

/// Ordered scalar lanes carried by a rectangular-pattern scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignRectangularPatternConstructionWire")]
pub(crate) struct DesignRectangularPatternConstruction {
    /// Positive U-direction instance count, including the seed.
    u_count: NonZeroU32,
    /// Positive V-direction instance count, including the seed.
    v_count: NonZeroU32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub(crate) owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instances: Option<DesignRectangularPatternInstances>,
}

#[cfg(test)]
thread_local! {
    static RECTANGULAR_PATTERN_CONSTRUCTION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignRectangularPatternConstruction {
    fn clone(&self) -> Self {
        #[cfg(test)]
        RECTANGULAR_PATTERN_CONSTRUCTION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            u_count: self.u_count,
            v_count: self.v_count,
            u_extent: self.u_extent,
            v_extent: self.v_extent,
            owner_record_indices: self.owner_record_indices,
            value_offsets: self.value_offsets,
            instances: self.instances.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignRectangularPatternConstructionRef<'a> {
    u_count: u32,
    v_count: u32,
    u_extent: f64,
    v_extent: f64,
    owner_record_indices: [u32; 4],
    value_offsets: [u64; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    instances: Option<&'a DesignRectangularPatternInstances>,
}

impl Serialize for DesignRectangularPatternConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignRectangularPatternConstructionRef {
            u_count: self.u_count.get(),
            v_count: self.v_count.get(),
            u_extent: self.u_extent,
            v_extent: self.v_extent,
            owner_record_indices: self.owner_record_indices,
            value_offsets: self.value_offsets,
            instances: self.instances.as_ref(),
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DesignRectangularPatternConstructionWire {
    /// Positive U-direction instance count, including the seed.
    pub(crate) u_count: u32,
    /// Positive V-direction instance count, including the seed.
    pub(crate) v_count: u32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    pub(crate) u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    pub(crate) v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub(crate) owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_instances"
    )]
    pub(crate) instances: Option<DesignRectangularPatternInstances>,
}

impl TryFrom<DesignRectangularPatternConstructionWire> for DesignRectangularPatternConstruction {
    type Error = &'static str;
    fn try_from(wire: DesignRectangularPatternConstructionWire) -> Result<Self, Self::Error> {
        let u_count = NonZeroU32::new(wire.u_count).ok_or("u_count must be nonzero")?;
        let v_count = NonZeroU32::new(wire.v_count).ok_or("v_count must be nonzero")?;
        if u_count.get() == 1 && v_count.get() == 1 {
            return Err("u_count and v_count must not both be one");
        }
        if !wire.u_extent.is_finite() || (u_count.get() == 1) != (wire.u_extent == 0.0) {
            return Err("u_extent must be finite and zero exactly when u_count is one");
        }
        if !wire.v_extent.is_finite() || (v_count.get() == 1) != (wire.v_extent == 0.0) {
            return Err("v_extent must be finite and zero exactly when v_count is one");
        }
        let mut record = Self {
            u_count,
            v_count,
            u_extent: wire.u_extent,
            v_extent: wire.v_extent,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
            instances: None,
        };
        record.try_set_instances(wire.instances)?;
        Ok(record)
    }
}

#[cfg(test)]
impl From<DesignRectangularPatternConstruction> for DesignRectangularPatternConstructionWire {
    fn from(value: DesignRectangularPatternConstruction) -> Self {
        Self {
            u_count: value.u_count.get(),
            v_count: value.v_count.get(),
            u_extent: value.u_extent,
            v_extent: value.v_extent,
            owner_record_indices: value.owner_record_indices,
            value_offsets: value.value_offsets,
            instances: value.instances,
        }
    }
}

impl DesignRectangularPatternConstruction {
    pub(crate) fn instances(&self) -> Option<&DesignRectangularPatternInstances> {
        self.instances.as_ref()
    }

    pub(crate) fn try_set_instances(
        &mut self,
        instances: Option<DesignRectangularPatternInstances>,
    ) -> Result<(), &'static str> {
        if let Some(run) = &instances {
            if (self.u_count.get() > 1) == (self.v_count.get() > 1) {
                return Err("instances require exactly one active axis");
            }
            let total = u64::from(self.u_count.get()) * u64::from(self.v_count.get());
            if u64::try_from(run.instance_count()) != Ok(total) {
                return Err("instances must match the positive total scalar count");
            }
        }
        self.instances = instances;
        Ok(())
    }

    pub(crate) fn u_count(&self) -> u32 {
        self.u_count.get()
    }
    pub(crate) fn v_count(&self) -> u32 {
        self.v_count.get()
    }
    pub(crate) fn u_extent(&self) -> f64 {
        self.u_extent
    }
    pub(crate) fn v_extent(&self) -> f64 {
        self.v_extent
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignRectangularPatternInstancesWire")]
pub(crate) enum DesignRectangularPatternInstances {
    Bodies(Vec<DesignPatternInstance>),
    Components {
        component_guid: DesignRelaxedGuidText,
        seed: DesignPatternComponentInstance,
        generated: Vec<DesignPatternComponentInstance>,
    },
}

#[cfg(test)]
thread_local! {
    static RECTANGULAR_PATTERN_INSTANCES_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignRectangularPatternInstances {
    fn clone(&self) -> Self {
        #[cfg(test)]
        RECTANGULAR_PATTERN_INSTANCES_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        match self {
            Self::Bodies(instances) => Self::Bodies(instances.clone()),
            Self::Components {
                component_guid,
                seed,
                generated,
            } => Self::Components {
                component_guid: component_guid.clone(),
                seed: seed.clone(),
                generated: generated.clone(),
            },
        }
    }
}

struct PatternRecordIndices<'a>(&'a DesignRectangularPatternInstances);
struct PatternTransforms<'a>(&'a DesignRectangularPatternInstances);
struct PatternTransformOffsets<'a>(&'a DesignRectangularPatternInstances);
struct PatternGeneratedGuids<'a>(&'a [DesignPatternComponentInstance]);

impl Serialize for PatternRecordIndices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.frames().map(|frame| frame.record_index))
    }
}

impl Serialize for PatternTransforms<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.frames().map(|frame| &frame.transform.value))
    }
}

impl Serialize for PatternTransformOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.frames().map(|frame| frame.transform.offset))
    }
}

impl Serialize for PatternGeneratedGuids<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|instance| &instance.occurrence_guid))
    }
}

#[derive(Serialize)]
struct DesignComponentPatternOccurrencesRef<'a> {
    component_guid: &'a DesignRelaxedGuidText,
    seed_occurrence_guid: &'a DesignRelaxedGuidText,
    generated_occurrence_guids: PatternGeneratedGuids<'a>,
}

#[derive(Serialize)]
struct DesignRectangularPatternInstancesRef<'a> {
    record_indices: PatternRecordIndices<'a>,
    transforms: PatternTransforms<'a>,
    transform_offsets: PatternTransformOffsets<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    component_occurrences: Option<DesignComponentPatternOccurrencesRef<'a>>,
}

impl Serialize for DesignRectangularPatternInstances {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let component_occurrences = match self {
            Self::Bodies(_) => None,
            Self::Components {
                component_guid,
                seed,
                generated,
            } => Some(DesignComponentPatternOccurrencesRef {
                component_guid,
                seed_occurrence_guid: &seed.occurrence_guid,
                generated_occurrence_guids: PatternGeneratedGuids(generated),
            }),
        };
        DesignRectangularPatternInstancesRef {
            record_indices: PatternRecordIndices(self),
            transforms: PatternTransforms(self),
            transform_offsets: PatternTransformOffsets(self),
            component_occurrences,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignPatternInstance {
    pub(crate) record_index: u32,
    pub(crate) transform: Located<SketchPlacementMatrix>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignPatternComponentInstance {
    pub(crate) instance: DesignPatternInstance,
    pub(crate) occurrence_guid: DesignRelaxedGuidText,
}

impl DesignRectangularPatternInstances {
    pub(crate) fn instance_count(&self) -> usize {
        match self {
            Self::Bodies(instances) => instances.len(),
            Self::Components { generated, .. } => generated.len() + 1,
        }
    }

    pub(crate) fn frames(&self) -> impl DoubleEndedIterator<Item = &DesignPatternInstance> {
        let (bodies, seed, generated): (
            &[DesignPatternInstance],
            Option<&DesignPatternInstance>,
            &[DesignPatternComponentInstance],
        ) = match self {
            Self::Bodies(instances) => (instances, None, &[]),
            Self::Components {
                seed, generated, ..
            } => (&[], Some(&seed.instance), generated),
        };
        bodies
            .iter()
            .chain(seed)
            .chain(generated.iter().map(|row| &row.instance))
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DesignRectangularPatternInstancesWire {
    /// Seed record followed by the generated-instance records in pattern order.
    record_indices: Vec<u32>,
    /// Row-major local-to-model placements parallel to `record_indices`.
    transforms: Vec<SketchPlacementMatrix>,
    /// Byte offsets of the first transform scalar parallel to `record_indices`.
    transform_offsets: Vec<u64>,
    /// Component occurrences carried by this run when the pattern repeats a component.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_occurrences"
    )]
    component_occurrences: Option<DesignComponentPatternOccurrencesWire>,
}

/// Component seed and generated occurrences carried by a rectangular pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentPatternOccurrencesWire {
    /// Reusable local component definition shared by every occurrence.
    component_guid: DesignRelaxedGuidText,
    /// Existing seed occurrence.
    seed_occurrence_guid: DesignRelaxedGuidText,
    /// Newly generated occurrences in pattern order after the seed.
    generated_occurrence_guids: Vec<DesignRelaxedGuidText>,
}

impl TryFrom<DesignRectangularPatternInstancesWire> for DesignRectangularPatternInstances {
    type Error = String;
    fn try_from(wire: DesignRectangularPatternInstancesWire) -> Result<Self, Self::Error> {
        if wire.record_indices.len() != wire.transforms.len()
            || wire.record_indices.len() != wire.transform_offsets.len()
        {
            return Err(
                "record_indices, transforms, and transform_offsets must have equal lengths".into(),
            );
        }
        let frames = wire
            .record_indices
            .into_iter()
            .zip(wire.transforms)
            .zip(wire.transform_offsets)
            .map(|((record_index, value), offset)| DesignPatternInstance {
                record_index,
                transform: Located { value, offset },
            });
        let Some(component) = wire.component_occurrences else {
            return Ok(Self::Bodies(frames.collect()));
        };
        let mut frames = frames;
        let seed = frames
            .next()
            .ok_or("component_occurrences requires a seed frame")?;
        if frames.len() != component.generated_occurrence_guids.len() {
            return Err(
                "generated_occurrence_guids must match the generated instance frames".into(),
            );
        }
        Ok(Self::Components {
            component_guid: component.component_guid,
            seed: DesignPatternComponentInstance {
                instance: seed,
                occurrence_guid: component.seed_occurrence_guid,
            },
            generated: frames
                .zip(component.generated_occurrence_guids)
                .map(
                    |(instance, occurrence_guid)| DesignPatternComponentInstance {
                        instance,
                        occurrence_guid,
                    },
                )
                .collect(),
        })
    }
}

#[cfg(test)]
impl From<DesignRectangularPatternInstances> for DesignRectangularPatternInstancesWire {
    fn from(instances: DesignRectangularPatternInstances) -> Self {
        let record_indices = instances.frames().map(|row| row.record_index).collect();
        let transforms = instances.frames().map(|row| row.transform.value).collect();
        let transform_offsets = instances.frames().map(|row| row.transform.offset).collect();
        let component_occurrences = match instances {
            DesignRectangularPatternInstances::Bodies(_) => None,
            DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            } => Some(DesignComponentPatternOccurrencesWire {
                component_guid,
                seed_occurrence_guid: seed.occurrence_guid,
                generated_occurrence_guids: generated
                    .into_iter()
                    .map(|row| row.occurrence_guid)
                    .collect(),
            }),
        };
        Self {
            record_indices,
            transforms,
            transform_offsets,
            component_occurrences,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesignAxis, DesignCircularPatternAxis, DesignCircularPatternAxisWire,
        DesignPatternAxisWrapper, DesignPatternComponentInstance, DesignPatternInstance,
        DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
        DesignRectangularPatternInstances, DesignRectangularPatternInstancesWire,
        CIRCULAR_PATTERN_AXIS_CLONE_COUNT, RECTANGULAR_PATTERN_CONSTRUCTION_CLONE_COUNT,
        RECTANGULAR_PATTERN_INSTANCES_CLONE_COUNT,
    };
    use crate::records::identity::Located;
    use crate::records::sketch_placement::SketchPlacementMatrix;
    use std::num::NonZeroU32;

    #[derive(serde::Serialize)]
    struct NestedRecord<'a, T: serde::Serialize> {
        id: &'static str,
        value: &'a T,
    }

    fn frame(index: u32) -> DesignPatternInstance {
        DesignPatternInstance {
            record_index: index,
            transform: Located {
                value: SketchPlacementMatrix::IDENTITY,
                offset: u64::from(index) * 10,
            },
        }
    }

    fn components() -> DesignRectangularPatternInstances {
        DesignRectangularPatternInstances::Components {
            component_guid: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .unwrap(),
            seed: DesignPatternComponentInstance {
                instance: frame(1),
                occurrence_guid: "22222222-2222-4222-8222-222222222222"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            },
            generated: vec![DesignPatternComponentInstance {
                instance: frame(2),
                occurrence_guid: "33333333-3333-4333-8333-333333333333"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            }],
        }
    }

    fn construction(
        instances: Option<DesignRectangularPatternInstances>,
    ) -> DesignRectangularPatternConstruction {
        DesignRectangularPatternConstruction {
            u_count: NonZeroU32::new(2).unwrap(),
            v_count: NonZeroU32::new(1).unwrap(),
            u_extent: 10.0,
            v_extent: 0.0,
            owner_record_indices: [1, 2, 3, 4],
            value_offsets: [10, 20, 30, 40],
            instances,
        }
    }

    fn circular_axis(resolved: bool) -> DesignCircularPatternAxis {
        DesignCircularPatternAxis::HistoricalEdge {
            wrappers: vec![
                DesignPatternAxisWrapper {
                    record_index: 1,
                    identity_offset: 20,
                },
                DesignPatternAxisWrapper {
                    record_index: 2,
                    identity_offset: 30,
                },
            ]
            .try_into()
            .unwrap(),
            persistent_identity: 7,
            resolved: resolved.then(|| DesignAxis {
                origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    0.0, 0.0, 0.0,
                ))
                .unwrap(),
                direction: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                    0.0, 0.0, 1.0,
                ))
                .unwrap(),
            }),
        }
    }

    #[test]
    fn circular_pattern_axis_borrowed_wire_matches_owned_wire_bytes() {
        let zero = cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap();
        let inline = DesignCircularPatternAxis::Inline {
            origin: [zero; 3],
            origin_offset: 10,
            direction: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .unwrap(),
            direction_offset: 20,
        };
        for axis in [inline, circular_axis(false), circular_axis(true)] {
            let owned = DesignCircularPatternAxisWire::from(axis.clone());
            assert_eq!(
                serde_json::to_vec(&axis).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn circular_pattern_axis_native_retained_limit_refuses_before_clone() {
        let axis = circular_axis(true);
        let record = NestedRecord {
            id: "f3d:native:circular-pattern-axis#0",
            value: &axis,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || CIRCULAR_PATTERN_AXIS_CLONE_COUNT.with(|count| count.set(0)),
            || CIRCULAR_PATTERN_AXIS_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn rectangular_pattern_instances_borrowed_wire_matches_owned_wire_bytes() {
        for instances in [
            DesignRectangularPatternInstances::Bodies(vec![frame(1), frame(2)]),
            components(),
        ] {
            let owned = DesignRectangularPatternInstancesWire::from(instances.clone());
            assert_eq!(
                serde_json::to_vec(&instances).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn rectangular_pattern_instances_native_retained_limit_refuses_before_clone() {
        let instances = components();
        let record = NestedRecord {
            id: "f3d:native:pattern-instances#0",
            value: &instances,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || RECTANGULAR_PATTERN_INSTANCES_CLONE_COUNT.with(|count| count.set(0)),
            || RECTANGULAR_PATTERN_INSTANCES_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn rectangular_pattern_construction_borrowed_wire_matches_owned_wire_bytes() {
        for record in [construction(None), construction(Some(components()))] {
            let owned = DesignRectangularPatternConstructionWire::from(record.clone());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn rectangular_pattern_construction_native_retained_limit_refuses_before_clone() {
        let construction = construction(Some(components()));
        let record = NestedRecord {
            id: "f3d:native:pattern-construction#0",
            value: &construction,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || RECTANGULAR_PATTERN_CONSTRUCTION_CLONE_COUNT.with(|count| count.set(0)),
            || RECTANGULAR_PATTERN_CONSTRUCTION_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn historical_pattern_axis_refuses_empty_wrapper_arrays() {
        let mut wire = serde_json::to_value(circular_axis(false)).unwrap();
        wire["wrapper_record_indices"] = serde_json::json!([]);
        wire["identity_offsets"] = serde_json::json!([]);
        let error = serde_json::from_value::<DesignCircularPatternAxis>(wire).unwrap_err();
        assert!(error.to_string().contains("wrappers must be nonempty"));
    }

    #[test]
    fn rectangular_pattern_instances_validate_scalar_count_and_active_axis() {
        for count in [0, 1, 3] {
            let mut record = construction(Some(components()));
            let previous = serde_json::to_vec(&record).unwrap();
            let frames = (0..count).map(frame).collect();
            assert!(record
                .try_set_instances(Some(DesignRectangularPatternInstances::Bodies(frames)))
                .is_err());
            assert_eq!(serde_json::to_vec(&record).unwrap(), previous);
            let mut wire = serde_json::to_value(&record).unwrap();
            wire["instances"] = serde_json::to_value(DesignRectangularPatternInstances::Bodies(
                (0..count).map(frame).collect(),
            ))
            .unwrap();
            assert!(serde_json::from_value::<DesignRectangularPatternConstruction>(wire).is_err());
        }
        let mut wire = DesignRectangularPatternConstructionWire::from(construction(None));
        wire.v_count = 2;
        wire.v_extent = 1.0;
        wire.instances = Some(DesignRectangularPatternInstances::Bodies(vec![
            frame(1),
            frame(2),
            frame(3),
            frame(4),
        ]));
        assert!(DesignRectangularPatternConstruction::try_from(wire).is_err());
    }
}

mod identity_rewrite;
