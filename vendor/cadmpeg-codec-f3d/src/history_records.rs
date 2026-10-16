// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion ASM construction-history record shapes.

use cadmpeg_ir::native::bytes::NativeBytes;

use serde::{Deserialize, Serialize};

use cadmpeg_ir::math::{Point3, Vector3};

/// Stream-size and history-entry-count pair from an ASM history preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AsmPreamble {
    pub(crate) stream_size: i64,
    pub(crate) history_entry_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AsmHistorySerde", into = "AsmHistorySerde")]
pub(crate) struct AsmHistory {
    pub(crate) id: String,
    pub(crate) byte_offset: u64,
    pub(crate) preamble: Option<AsmPreamble>,
    /// True when historical topology binding was not attempted because its
    /// state-by-record work estimate exceeded the decoder safety budget.
    pub(crate) record_table_binding_budget_exceeded: bool,
    pub(crate) states: Vec<AsmDeltaState>,
}

impl AsmHistory {
    /// Historical projection consumers finished and every temporary complete
    /// topology snapshot this history holds was released. A history with no
    /// states has released nothing and is not finalized.
    pub(crate) fn projection_finalized(&self) -> bool {
        !self.states.is_empty()
            && self
                .states
                .iter()
                .all(super::history_records::AsmDeltaState::projection_released)
    }

    pub(crate) fn stream_size(&self) -> Option<i64> {
        self.preamble.map(|preamble| preamble.stream_size)
    }

    pub(crate) fn history_entry_count(&self) -> Option<i64> {
        self.preamble.map(|preamble| preamble.history_entry_count)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AsmHistorySerde {
    id: String,
    byte_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stream_size"
    )]
    stream_size: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_history_entry_count"
    )]
    #[serde(alias = "high_water_mark")]
    history_entry_count: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    record_table_binding_budget_exceeded: bool,
    states: Vec<AsmDeltaState>,
}

impl TryFrom<AsmHistorySerde> for AsmHistory {
    type Error = String;

    fn try_from(wire: AsmHistorySerde) -> Result<Self, Self::Error> {
        let preamble = match (wire.stream_size, wire.history_entry_count) {
            (Some(stream_size), Some(history_entry_count)) => Some(AsmPreamble {
                stream_size,
                history_entry_count,
            }),
            (None, None) => None,
            _ => {
                return Err(
                    "asm history stream_size and history_entry_count must be paired".into(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            preamble,
            record_table_binding_budget_exceeded: wire.record_table_binding_budget_exceeded,
            states: wire.states,
        })
    }
}

impl From<AsmHistory> for AsmHistorySerde {
    fn from(history: AsmHistory) -> Self {
        let stream_size = history.stream_size();
        let history_entry_count = history.history_entry_count();
        Self {
            id: history.id,
            byte_offset: history.byte_offset,
            stream_size,
            history_entry_count,
            record_table_binding_budget_exceeded: history.record_table_binding_budget_exceeded,
            states: history.states,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AsmDeltaStateWire", into = "AsmDeltaStateWire")]
pub(crate) struct AsmDeltaState {
    pub(crate) id: String,
    pub(crate) parent: String,
    pub(crate) byte_offset: u64,
    pub(crate) state_id: i64,
    pub(crate) version_flag: i64,
    pub(crate) state_flag: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) next_ref: Option<i64>,
    pub(crate) node_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) partner_ref: Option<i64>,
    pub(crate) owner_ref: i64,
    #[serde(default)]
    pub(crate) bulletin_boards: Vec<AsmBulletinBoard>,
    #[serde(default)]
    pub(crate) records: Vec<AsmHistoryRecord>,
    /// Topology-entity slot to record-revision map at this state. The decoder
    /// retains this compact map for late persistent-selection binding after
    /// projection caches are finalized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) entity_versions: Vec<AsmEntityVersion>,
    pub(crate) topology_cache: AsmTopologyCache,
    /// Forward change from the state reached by `next_ref` to this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transition: Option<AsmHistoricalTransition>,
}

/// Historical topology retained for projection or for late identity resolution.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum AsmTopologyCache {
    #[default]
    Absent,
    Complete(AsmHistoricalTopology),
    Retained(AsmHistoricalTopology),
    /// The complete snapshot was released at finalization and nothing was kept.
    Released,
}

/// Serialized discriminant of one state's topology cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AsmTopologyCacheKind {
    #[default]
    Absent,
    Complete,
    Retained,
    Released,
}

/// Whether a serialized field still holds its default.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl AsmDeltaState {
    pub(crate) fn topology(&self) -> Option<&AsmHistoricalTopology> {
        match &self.topology_cache {
            AsmTopologyCache::Absent | AsmTopologyCache::Released => None,
            AsmTopologyCache::Complete(topology) | AsmTopologyCache::Retained(topology) => {
                Some(topology)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn topology_mut(&mut self) -> Option<&mut AsmHistoricalTopology> {
        match &mut self.topology_cache {
            AsmTopologyCache::Absent | AsmTopologyCache::Released => None,
            AsmTopologyCache::Complete(topology) | AsmTopologyCache::Retained(topology) => {
                Some(topology)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn record_table_complete(&self) -> bool {
        matches!(self.topology_cache, AsmTopologyCache::Complete(_))
    }

    /// The serialized discriminant of this state's topology cache.
    pub(crate) fn topology_cache_kind(&self) -> AsmTopologyCacheKind {
        match self.topology_cache {
            AsmTopologyCache::Absent => AsmTopologyCacheKind::Absent,
            AsmTopologyCache::Complete(_) => AsmTopologyCacheKind::Complete,
            AsmTopologyCache::Retained(_) => AsmTopologyCacheKind::Retained,
            AsmTopologyCache::Released => AsmTopologyCacheKind::Released,
        }
    }

    /// The state's complete projection snapshot was released at finalization.
    fn projection_released(&self) -> bool {
        matches!(
            self.topology_cache,
            AsmTopologyCache::Retained(_) | AsmTopologyCache::Released
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AsmDeltaStateWire {
    id: String,
    parent: String,
    byte_offset: u64,
    state_id: i64,
    version_flag: i64,
    state_flag: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_previous_ref"
    )]
    previous_ref: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_next_ref"
    )]
    next_ref: Option<i64>,
    node_index: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_partner_ref"
    )]
    partner_ref: Option<i64>,
    owner_ref: i64,
    #[serde(default)]
    bulletin_boards: Vec<AsmBulletinBoard>,
    #[serde(default)]
    records: Vec<AsmHistoryRecord>,
    /// Topology-entity slot to record-revision map at this state. The decoder
    /// retains this compact map for late persistent-selection binding after
    /// projection caches are finalized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entity_versions: Vec<AsmEntityVersion>,
    /// Which form of topology snapshot this state holds.
    #[serde(default, skip_serializing_if = "is_default")]
    topology_cache: AsmTopologyCacheKind,
    /// Stable `RecordTable` identities emitted by the ordinary B-rep decoder for
    /// this historical state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_topology"
    )]
    topology: Option<AsmHistoricalTopology>,
    /// Forward change from the state reached by `next_ref` to this state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transition"
    )]
    transition: Option<AsmHistoricalTransition>,
}

impl TryFrom<AsmDeltaStateWire> for AsmDeltaState {
    type Error = String;

    fn try_from(wire: AsmDeltaStateWire) -> Result<Self, Self::Error> {
        let topology_cache = match (wire.topology_cache, wire.topology) {
            (AsmTopologyCacheKind::Absent, None) => AsmTopologyCache::Absent,
            (AsmTopologyCacheKind::Released, None) => AsmTopologyCache::Released,
            (AsmTopologyCacheKind::Complete, Some(topology)) => {
                AsmTopologyCache::Complete(topology)
            }
            (AsmTopologyCacheKind::Retained, Some(topology)) => {
                AsmTopologyCache::Retained(topology)
            }
            (AsmTopologyCacheKind::Absent | AsmTopologyCacheKind::Released, Some(_)) => {
                return Err("topology_cache carries no topology in this form".into())
            }
            (AsmTopologyCacheKind::Complete | AsmTopologyCacheKind::Retained, None) => {
                return Err("topology_cache requires topology".into())
            }
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            byte_offset: wire.byte_offset,
            state_id: wire.state_id,
            version_flag: wire.version_flag,
            state_flag: wire.state_flag,
            previous_ref: wire.previous_ref,
            next_ref: wire.next_ref,
            node_index: wire.node_index,
            partner_ref: wire.partner_ref,
            owner_ref: wire.owner_ref,
            bulletin_boards: wire.bulletin_boards,
            records: wire.records,
            entity_versions: wire.entity_versions,
            transition: wire.transition,
            topology_cache,
        })
    }
}

impl From<AsmDeltaState> for AsmDeltaStateWire {
    fn from(state: AsmDeltaState) -> Self {
        let (topology_cache, topology) = match state.topology_cache {
            AsmTopologyCache::Absent => (AsmTopologyCacheKind::Absent, None),
            AsmTopologyCache::Released => (AsmTopologyCacheKind::Released, None),
            AsmTopologyCache::Complete(topology) => {
                (AsmTopologyCacheKind::Complete, Some(topology))
            }
            AsmTopologyCache::Retained(topology) => {
                (AsmTopologyCacheKind::Retained, Some(topology))
            }
        };
        Self {
            id: state.id,
            parent: state.parent,
            byte_offset: state.byte_offset,
            state_id: state.state_id,
            version_flag: state.version_flag,
            state_flag: state.state_flag,
            previous_ref: state.previous_ref,
            next_ref: state.next_ref,
            node_index: state.node_index,
            partner_ref: state.partner_ref,
            owner_ref: state.owner_ref,
            bulletin_boards: state.bulletin_boards,
            records: state.records,
            entity_versions: state.entity_versions,
            transition: state.transition,
            topology_cache,
            topology,
        }
    }
}

/// Record revision occupying one stable entity slot at an ASM history state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmEntityVersion {
    pub(crate) entity_ref: i64,
    pub(crate) record_ref: i64,
}

/// Stable entity-slot membership of one re-derived historical B-rep.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalTopology {
    pub(crate) bodies: Vec<i64>,
    pub(crate) regions: Vec<i64>,
    pub(crate) shells: Vec<i64>,
    pub(crate) faces: Vec<i64>,
    pub(crate) loops: Vec<i64>,
    pub(crate) coedges: Vec<i64>,
    pub(crate) edges: Vec<i64>,
    pub(crate) vertices: Vec<i64>,
    pub(crate) points: Vec<i64>,
    pub(crate) surfaces: Vec<i64>,
    /// Characteristic radii of analytic or constant-radius blend carriers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) surface_radii: Vec<AsmHistoricalSurfaceRadius>,
    /// Exact right-circular cylinder carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) surface_cylinders: Vec<AsmHistoricalCylinder>,
    /// Exact plane carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) surface_planes: Vec<AsmHistoricalPlane>,
    /// Model-space axes of axis-bearing analytic surface carriers in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) surface_axes: Vec<AsmHistoricalSurfaceAxis>,
    pub(crate) curves: Vec<i64>,
    /// Model-space axes of axis-bearing curve carriers in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) curve_axes: Vec<AsmHistoricalCurveAxis>,
    pub(crate) pcurves: Vec<i64>,
    /// Persistent tag groups attached to face and edge revisions in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) persistent_subentity_tags: Vec<AsmHistoricalPersistentSubentityTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body_regions: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) region_shells: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shell_faces: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shell_wire_edges: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shell_free_vertices: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) face_loops: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) loop_coedges: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) coedge_topology: Vec<AsmHistoricalCoedge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) edge_vertices: Vec<AsmHistoricalEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) face_surfaces: Vec<AsmHistoricalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) edge_curves: Vec<AsmHistoricalOptionalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) coedge_pcurves: Vec<AsmHistoricalOptionalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) vertex_points: Vec<AsmHistoricalCarrierBinding>,
    /// Model-space values of the point carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) point_positions: Vec<AsmHistoricalPoint>,
}

/// One persistent tag group attached to a historical face or edge revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalPersistentSubentityTag {
    pub(crate) entity_kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind,
    pub(crate) entity_ref: i64,
    pub(crate) selector: i64,
    pub(crate) token: String,
    pub(crate) design_references: Vec<i64>,
    pub(crate) ordinal: u32,
}

/// Stable axis-bearing curve carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalCurveAxis {
    pub(crate) curve: i64,
    pub(crate) origin: Point3,
    pub(crate) direction: Vector3,
}

/// Stable axis line of one cylinder, cone, or torus carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalSurfaceAxis {
    pub(crate) surface: i64,
    pub(crate) origin: Point3,
    pub(crate) direction: Vector3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalSurfaceRadius {
    pub(crate) surface: i64,
    pub(crate) radius: f64,
}

/// Stable geometry of one right-circular cylinder carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalCylinder {
    pub(crate) surface: i64,
    pub(crate) origin: Point3,
    pub(crate) axis: Vector3,
    pub(crate) radius: f64,
}

/// Stable geometry of one plane carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalPlane {
    pub(crate) surface: i64,
    pub(crate) origin: Point3,
    pub(crate) normal: Vector3,
}

/// Stable point-carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalPoint {
    pub(crate) point: i64,
    pub(crate) position: Point3,
}

/// Ordered stable entity-slot relation in a historical B-rep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalRelation {
    pub(crate) owner_ref: i64,
    pub(crate) member_refs: Vec<i64>,
}

/// Stable topology links of one historical coedge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalCoedge {
    pub(crate) coedge: i64,
    pub(crate) owner_loop: i64,
    pub(crate) edge: i64,
    pub(crate) next: i64,
    pub(crate) previous: i64,
    pub(crate) radial_next: i64,
}

/// Ordered endpoint links of one historical edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalEdge {
    pub(crate) edge: i64,
    pub(crate) start_vertex: i64,
    pub(crate) end_vertex: i64,
}

/// Stable binding from a topology entity to its required geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalCarrierBinding {
    pub(crate) entity: i64,
    pub(crate) carrier: i64,
}

/// Stable binding from a topology entity to its optional geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalOptionalCarrierBinding {
    pub(crate) entity: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier"
    )]
    pub(crate) carrier: Option<i64>,
}

/// Forward stable-slot changes from an older ASM state to a newer state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalTransition {
    /// Older state identity; absent only at the end of the reverse-history chain.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_previous_state_id"
    )]
    pub(crate) previous_state_id: Option<i64>,
    /// Changes across the complete normalized `RecordTable`.
    pub(crate) records: AsmHistoricalEntityDelta,
    /// Changes restricted to each normalized topology family.
    pub(crate) topology: AsmHistoricalTopologyDelta,
}

/// Stable entity slots inserted, deleted, or assigned a different record revision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalEntityDelta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) inserted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) deleted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) updated: Vec<i64>,
}

/// Per-family topology changes between two complete historical states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AsmHistoricalTopologyDelta {
    pub(crate) bodies: AsmHistoricalEntityDelta,
    pub(crate) regions: AsmHistoricalEntityDelta,
    pub(crate) shells: AsmHistoricalEntityDelta,
    pub(crate) faces: AsmHistoricalEntityDelta,
    pub(crate) loops: AsmHistoricalEntityDelta,
    pub(crate) coedges: AsmHistoricalEntityDelta,
    pub(crate) edges: AsmHistoricalEntityDelta,
    pub(crate) vertices: AsmHistoricalEntityDelta,
    pub(crate) points: AsmHistoricalEntityDelta,
    pub(crate) surfaces: AsmHistoricalEntityDelta,
    pub(crate) curves: AsmHistoricalEntityDelta,
    pub(crate) pcurves: AsmHistoricalEntityDelta,
}

#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "AsmHistoryRecordWire")]
pub(crate) struct AsmHistoryRecord {
    pub(crate) id: String,
    pub(crate) parent: String,
    pub(crate) revision_id: Option<i64>,
    pub(crate) byte_offset: u64,
    pub(crate) framing: AsmHistoryRecordFraming,
    pub(crate) raw_bytes: NativeBytes<Vec<u8>>,
}

#[cfg(test)]
std::thread_local! {
    static HISTORY_RECORD_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for AsmHistoryRecord {
    fn clone(&self) -> Self {
        HISTORY_RECORD_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            parent: self.parent.clone(),
            revision_id: self.revision_id,
            byte_offset: self.byte_offset,
            framing: self.framing.clone(),
            raw_bytes: (self.raw_bytes.clone()),
        }
    }
}

#[derive(Serialize)]
struct AsmHistoryRecordWireRef<'a> {
    id: &'a str,
    parent: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_id: Option<i64>,
    index: u64,
    byte_offset: u64,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    framing_error: Option<&'a str>,
    #[serde(skip_serializing_if = "<[i64]>::is_empty")]
    entity_references: &'a [i64],
    raw_bytes: NativeBytes<&'a [u8]>,
}

impl Serialize for AsmHistoryRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (index, name, framing_error, entity_references) = match &self.framing {
            AsmHistoryRecordFraming::Framed {
                index,
                name,
                entity_references,
            } => (*index, name.as_str(), None, entity_references.as_slice()),
            AsmHistoryRecordFraming::Opaque { error } => {
                (0, "opaque_history_payload", Some(error.as_str()), &[][..])
            }
        };
        AsmHistoryRecordWireRef {
            id: &self.id,
            parent: &self.parent,
            revision_id: self.revision_id,
            index,
            byte_offset: self.byte_offset,
            name,
            framing_error,
            entity_references,
            raw_bytes: (&self.raw_bytes).into(),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AsmHistoryRecordFraming {
    Framed {
        index: u64,
        name: String,
        entity_references: Vec<i64>,
    },
    Opaque {
        error: String,
    },
}

impl AsmHistoryRecord {
    pub(crate) fn name(&self) -> &str {
        match &self.framing {
            AsmHistoryRecordFraming::Framed { name, .. } => name,
            AsmHistoryRecordFraming::Opaque { .. } => "opaque_history_payload",
        }
    }

    pub(crate) fn framing_error(&self) -> Option<&str> {
        match &self.framing {
            AsmHistoryRecordFraming::Framed { .. } => None,
            AsmHistoryRecordFraming::Opaque { error } => Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct AsmHistoryRecordWire {
    id: String,
    parent: String,
    /// Construction-history revision identity paired from the ordered
    /// old-reference run; absent only for the stream terminator or an opaque
    /// snapshot whose pairing cannot be established.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_revision_id"
    )]
    revision_id: Option<i64>,
    /// Snapshot-local record ordinal. This is not the revision identity.
    index: u64,
    /// Byte offset of the record in the decompressed ASM stream.
    #[serde(default)]
    byte_offset: u64,
    name: String,
    /// Framing failure that forced this span to remain opaque.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_framing_error"
    )]
    framing_error: Option<String>,
    /// Ordered `0x0c` entity-reference tokens in the history revision namespace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entity_references: Vec<i64>,
    raw_bytes: NativeBytes<Vec<u8>>,
}

impl TryFrom<AsmHistoryRecordWire> for AsmHistoryRecord {
    type Error = String;

    fn try_from(wire: AsmHistoryRecordWire) -> Result<Self, Self::Error> {
        let framing = match wire.framing_error {
            None => AsmHistoryRecordFraming::Framed {
                index: wire.index,
                name: wire.name,
                entity_references: wire.entity_references,
            },
            Some(error) => {
                if wire.index != 0
                    || wire.name != "opaque_history_payload"
                    || !wire.entity_references.is_empty()
                {
                    return Err("framing_error requires index 0, name opaque_history_payload, and empty entity_references".into());
                }
                AsmHistoryRecordFraming::Opaque { error }
            }
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            revision_id: wire.revision_id,
            byte_offset: wire.byte_offset,
            framing,
            raw_bytes: (wire.raw_bytes),
        })
    }
}

#[cfg(test)]
impl From<AsmHistoryRecord> for AsmHistoryRecordWire {
    fn from(record: AsmHistoryRecord) -> Self {
        let (index, name, framing_error, entity_references) = match record.framing {
            AsmHistoryRecordFraming::Framed {
                index,
                name,
                entity_references,
            } => (index, name, None, entity_references),
            AsmHistoryRecordFraming::Opaque { error } => {
                (0, "opaque_history_payload".into(), Some(error), Vec::new())
            }
        };
        Self {
            id: record.id,
            parent: record.parent,
            revision_id: record.revision_id,
            index,
            byte_offset: record.byte_offset,
            name,
            framing_error,
            entity_references,
            raw_bytes: (record.raw_bytes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmBulletinBoard {
    pub(crate) id: String,
    pub(crate) parent: String,
    pub(crate) byte_offset: u64,
    pub(crate) owner_ref: i64,
    pub(crate) number: i64,
    pub(crate) changes: Vec<AsmEntityChange>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "AsmEntityChangeSerde")]
pub(crate) struct AsmEntityChange {
    pub(crate) id: String,
    pub(crate) parent: String,
    pub(crate) byte_offset: u64,
    pub(crate) kind: AsmEntityChangeKind,
}

#[cfg(test)]
std::thread_local! {
    static ENTITY_CHANGE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for AsmEntityChange {
    fn clone(&self) -> Self {
        ENTITY_CHANGE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            parent: self.parent.clone(),
            byte_offset: self.byte_offset,
            kind: self.kind,
        }
    }
}

#[derive(Serialize)]
struct AsmEntityChangeWireRef<'a> {
    id: &'a str,
    parent: &'a str,
    byte_offset: u64,
    kind: AsmEntityChangeKindWire,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_ref: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_ref: Option<i64>,
}

impl Serialize for AsmEntityChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, old_ref, new_ref) = match self.kind {
            AsmEntityChangeKind::Insert { new } => {
                (AsmEntityChangeKindWire::Insert, None, Some(new))
            }
            AsmEntityChangeKind::Delete { old } => {
                (AsmEntityChangeKindWire::Delete, Some(old), None)
            }
            AsmEntityChangeKind::Update { old, new } => {
                (AsmEntityChangeKindWire::Update, Some(old), Some(new))
            }
        };
        AsmEntityChangeWireRef {
            id: &self.id,
            parent: &self.parent,
            byte_offset: self.byte_offset,
            kind,
            old_ref,
            new_ref,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AsmEntityChangeKind {
    Insert { new: i64 },
    Delete { old: i64 },
    Update { old: i64, new: i64 },
}

impl AsmEntityChange {
    pub(crate) fn old_ref(&self) -> Option<i64> {
        match self.kind {
            AsmEntityChangeKind::Insert { .. } => None,
            AsmEntityChangeKind::Delete { old } | AsmEntityChangeKind::Update { old, .. } => {
                Some(old)
            }
        }
    }

    pub(crate) fn new_ref(&self) -> Option<i64> {
        match self.kind {
            AsmEntityChangeKind::Delete { .. } => None,
            AsmEntityChangeKind::Insert { new } | AsmEntityChangeKind::Update { new, .. } => {
                Some(new)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct AsmEntityChangeSerde {
    id: String,
    parent: String,
    byte_offset: u64,
    kind: AsmEntityChangeKindWire,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_old_ref"
    )]
    old_ref: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_new_ref"
    )]
    new_ref: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AsmEntityChangeKindWire {
    Insert,
    Delete,
    Update,
}

impl TryFrom<AsmEntityChangeSerde> for AsmEntityChange {
    type Error = String;

    fn try_from(wire: AsmEntityChangeSerde) -> Result<Self, Self::Error> {
        let kind = match (wire.kind, wire.old_ref, wire.new_ref) {
            (AsmEntityChangeKindWire::Insert, None, Some(new)) => {
                AsmEntityChangeKind::Insert { new }
            }
            (AsmEntityChangeKindWire::Delete, Some(old), None) => {
                AsmEntityChangeKind::Delete { old }
            }
            (AsmEntityChangeKindWire::Update, Some(old), Some(new)) => {
                AsmEntityChangeKind::Update { old, new }
            }
            _ => {
                return Err("asm entity change kind disagrees with old_ref/new_ref".into());
            }
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            byte_offset: wire.byte_offset,
            kind,
        })
    }
}

#[cfg(test)]
impl From<AsmEntityChange> for AsmEntityChangeSerde {
    fn from(change: AsmEntityChange) -> Self {
        let (kind, old_ref, new_ref) = match change.kind {
            AsmEntityChangeKind::Insert { new } => {
                (AsmEntityChangeKindWire::Insert, None, Some(new))
            }
            AsmEntityChangeKind::Delete { old } => {
                (AsmEntityChangeKindWire::Delete, Some(old), None)
            }
            AsmEntityChangeKind::Update { old, new } => {
                (AsmEntityChangeKindWire::Update, Some(old), Some(new))
            }
        };
        Self {
            id: change.id,
            parent: change.parent,
            byte_offset: change.byte_offset,
            kind,
            old_ref,
            new_ref,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AsmDeltaState, AsmHistoricalTopology, AsmTopologyCache};

    #[test]
    fn history_record_borrowed_wire_matches_owned_wire_bytes() {
        use super::{AsmHistoryRecord, AsmHistoryRecordFraming, AsmHistoryRecordWire};

        for framing in [
            AsmHistoryRecordFraming::Framed {
                index: 4,
                name: "edge".into(),
                entity_references: vec![7, -1],
            },
            AsmHistoryRecordFraming::Opaque {
                error: "invalid frame".into(),
            },
        ] {
            let record = AsmHistoryRecord {
                id: "f3d:native:history_record#1".into(),
                parent: "f3d:native:state#1".into(),
                revision_id: Some(7),
                byte_offset: 12,
                framing,
                raw_bytes: (b"history payload".to_vec()).into(),
            };
            let old = AsmHistoryRecordWire::from(record.clone());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&old).unwrap()
            );
        }
    }

    #[test]
    fn history_record_native_retained_limit_refuses_before_raw_bytes_clone() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let record = super::AsmHistoryRecord {
            id: "f3d:native:history_record#1".into(),
            parent: "f3d:native:state#1".into(),
            revision_id: Some(7),
            byte_offset: 12,
            framing: super::AsmHistoryRecordFraming::Framed {
                index: 4,
                name: "edge".into(),
                entity_references: vec![7, -1],
            },
            raw_bytes: (vec![0xab; 4096]).into(),
        };
        let needed = serde_json::to_vec(&record).unwrap().len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        super::HISTORY_RECORD_CLONE_COUNT.with(|count| count.set(0));
        let error = namespace
            .set_arena(
                &limited,
                "asm_history_records",
                std::slice::from_ref(&record),
            )
            .unwrap_err();
        super::HISTORY_RECORD_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        assert!(matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "serialize native record"
        ));

        let (service, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        namespace
            .set_arena(
                &service,
                "asm_history_records",
                std::slice::from_ref(&record),
            )
            .unwrap();
        let stored = &namespace.arenas()["asm_history_records"][0];
        assert_eq!(
            serde_json::to_value(stored).unwrap(),
            serde_json::to_value(&record).unwrap()
        );
    }

    #[test]
    fn entity_change_borrowed_wire_matches_owned_wire_bytes() {
        use super::{AsmEntityChange, AsmEntityChangeKind, AsmEntityChangeSerde};

        for kind in [
            AsmEntityChangeKind::Insert { new: 7 },
            AsmEntityChangeKind::Delete { old: 8 },
            AsmEntityChangeKind::Update { old: 8, new: 7 },
        ] {
            let change = AsmEntityChange {
                id: "f3d:native:entity_change#1".into(),
                parent: "f3d:native:bulletin#1".into(),
                byte_offset: 12,
                kind,
            };
            let owned = AsmEntityChangeSerde::from(change.clone());
            assert_eq!(
                serde_json::to_vec(&change).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn entity_change_native_retained_limit_refuses_before_string_clone() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let change = super::AsmEntityChange {
            id: "f3d:native:entity_change#1".into(),
            parent: "f3d:native:bulletin#1".into(),
            byte_offset: 12,
            kind: super::AsmEntityChangeKind::Update { old: 8, new: 7 },
        };
        let needed = "asm_entity_changes".len()
            + 4 * std::mem::size_of::<cadmpeg_ir::NativeRecord>()
            + "id".len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        super::ENTITY_CHANGE_CLONE_COUNT.with(|count| count.set(0));
        let error = namespace
            .set_arena(
                &limited,
                "asm_entity_changes",
                std::slice::from_ref(&change),
            )
            .unwrap_err();
        super::ENTITY_CHANGE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        assert!(matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "serialize native record"
        ));

        let (service, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        namespace
            .set_arena(
                &service,
                "asm_entity_changes",
                std::slice::from_ref(&change),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&namespace.arenas()["asm_entity_changes"][0]).unwrap(),
            serde_json::to_value(&change).unwrap()
        );
    }

    #[test]
    fn topology_cache_wire_preserves_every_form_and_rejects_a_missing_topology() {
        let prefix = r#"{"id":"state","parent":"history","byte_offset":0,"state_id":1,"version_flag":1,"state_flag":0,"node_index":1,"owner_ref":0,"bulletin_boards":[],"records":[]"#;
        let topology = serde_json::to_string(&AsmHistoricalTopology::default()).unwrap();
        for (complete, released, has_topology, fields) in [
            (false, false, false, String::new()),
            (
                true,
                false,
                true,
                format!(",\"topology_cache\":\"complete\",\"topology\":{topology}"),
            ),
            (
                false,
                true,
                true,
                format!(",\"topology_cache\":\"retained\",\"topology\":{topology}"),
            ),
            (
                false,
                true,
                false,
                ",\"topology_cache\":\"released\"".to_owned(),
            ),
        ] {
            let wire = format!("{prefix}{fields}}}");
            let state: AsmDeltaState = serde_json::from_str(&wire).unwrap();
            assert_eq!(state.record_table_complete(), complete);
            assert_eq!(state.topology().is_some(), has_topology);
            assert_eq!(state.projection_released(), released);
            let expected = match &state.topology_cache {
                AsmTopologyCache::Absent => (false, false),
                AsmTopologyCache::Complete(_) => (true, false),
                AsmTopologyCache::Retained(_) | AsmTopologyCache::Released => (false, true),
            };
            assert_eq!(expected, (complete, released));
            assert_eq!(serde_json::to_string(&state).unwrap(), wire);
        }
        let invalid = format!("{prefix},\"topology_cache\":\"complete\"}}");
        let error = serde_json::from_str::<AsmDeltaState>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("topology_cache"));
        assert!(error.contains("topology"));
    }
    #[test]
    fn history_record_framing_preserves_wire_and_rejects_opaque_metadata() {
        let prefix =
            r#"{"id":"record","parent":"state","revision_id":7,"index":0,"byte_offset":12,"name":"#;
        for fields in [
            r#""edge","entity_references":[7,-1]"#,
            r#""opaque_history_payload","framing_error":"invalid frame""#,
        ] {
            let wire = format!("{prefix}{fields},\"raw_bytes\":\"\"}}");
            let record: super::AsmHistoryRecord = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        }
        for fields in [
            r#""edge","framing_error":"invalid frame""#,
            r#""opaque_history_payload","framing_error":"invalid frame","entity_references":[7]"#,
        ] {
            let wire = format!("{prefix}{fields},\"raw_bytes\":\"\"}}");
            let error = serde_json::from_str::<super::AsmHistoryRecord>(&wire)
                .unwrap_err()
                .to_string();
            assert!(error.contains("framing_error"));
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_stream_size, i64, "stream_size");
cadmpeg_core::named_optional_field!(deserialize_history_entry_count, i64, "history_entry_count");
cadmpeg_core::named_optional_field!(deserialize_previous_ref, i64, "previous_ref");
cadmpeg_core::named_optional_field!(deserialize_next_ref, i64, "next_ref");
cadmpeg_core::named_optional_field!(deserialize_partner_ref, i64, "partner_ref");
cadmpeg_core::named_optional_field!(deserialize_topology, AsmHistoricalTopology, "topology");
cadmpeg_core::named_optional_field!(
    deserialize_transition,
    AsmHistoricalTransition,
    "transition"
);
cadmpeg_core::named_optional_field!(deserialize_carrier, i64, "carrier");
cadmpeg_core::named_optional_field!(deserialize_previous_state_id, i64, "previous_state_id");
cadmpeg_core::named_optional_field!(deserialize_revision_id, i64, "revision_id");
cadmpeg_core::named_optional_field!(deserialize_framing_error, String, "framing_error");
cadmpeg_core::named_optional_field!(deserialize_old_ref, i64, "old_ref");
cadmpeg_core::named_optional_field!(deserialize_new_ref, i64, "new_ref");
