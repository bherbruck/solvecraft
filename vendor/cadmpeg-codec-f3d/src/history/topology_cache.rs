// SPDX-License-Identifier: Apache-2.0
//! Cache the topology indexes used by historical face and edge recipes.
use super::cache::SnapshotCache;
use super::loop_index::LoopIndex;
use crate::history_records::AsmHistoricalTopology;
use std::collections::{HashMap, HashSet};

pub(super) type EdgeEndpoints = HashMap<i64, Option<[i64; 2]>>;

pub(super) type BoundaryEdges = HashMap<i64, HashSet<i64>>;

#[derive(Default)]
pub(super) struct TopologyQueryCache<'a> {
    pub(super) vertices: SnapshotCache<'a, AsmHistoricalTopology, VertexBoundaryIndex>,
    pub(super) endpoints: SnapshotCache<'a, AsmHistoricalTopology, EdgeEndpoints>,
    pub(super) boundaries: SnapshotCache<'a, AsmHistoricalTopology, BoundaryEdges>,
    pub(super) loops: SnapshotCache<'a, AsmHistoricalTopology, LoopIndex<'a>>,
}

pub(super) struct VertexBoundaryIndex {
    pub(super) boundaries: BoundaryEdges,
    pub(super) vertices: HashSet<i64>,
    pub(super) faces: HashSet<i64>,
}
impl VertexBoundaryIndex {
    pub(super) fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        topology: &AsmHistoricalTopology,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let boundaries = super::face_boundary_edge_index(ctx, topology)?;
        let vertices = ctx.collect_hash_set(
            topology.vertices.iter().copied(),
            "index F3D boundary live vertices",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(topology.vertices.len()),
            "index F3D boundary live vertices",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(topology.faces.len()),
            "index F3D boundary live faces",
        )?;
        let faces = ctx.collect_hash_set(
            topology.faces.iter().copied(),
            "index F3D boundary live faces",
        )?;
        Ok(Self {
            boundaries,
            vertices,
            faces,
        })
    }
}

pub(super) fn endpoint_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<EdgeEndpoints, cadmpeg_core::CodecError> {
    let mut endpoints = HashMap::new();
    for edge in &topology.edge_vertices {
        ctx.charge_work(1, "index F3D boundary vertex endpoints")?;
        if !endpoints.contains_key(&edge.edge) {
            ctx.reserve_map(&mut endpoints, 1, "index F3D boundary vertex endpoints")?;
        }
        endpoints
            .entry(edge.edge)
            .and_modify(|value| *value = None)
            .or_insert(Some([edge.start_vertex, edge.end_vertex]));
    }
    Ok(endpoints)
}

pub(super) fn aggregate_boundaries(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<BoundaryEdges, cadmpeg_core::CodecError> {
    let mut loop_coedges = HashMap::new();
    for relation in &topology.loop_coedges {
        for member in &relation.member_refs {
            ctx.charge_work(1, "index F3D aggregate boundary loops")?;
            ctx.push_hash_group(
                &mut loop_coedges,
                relation.owner_ref,
                *member,
                "index F3D aggregate boundary loops",
                "collect F3D aggregate boundary coedges",
            )?;
        }
    }
    let mut coedge_edges = HashMap::new();
    for coedge in &topology.coedge_topology {
        ctx.charge_work(1, "index F3D aggregate boundary coedges")?;
        ctx.push_hash_group(
            &mut coedge_edges,
            coedge.coedge,
            coedge.edge,
            "index F3D aggregate boundary coedges",
            "collect F3D aggregate boundary edges",
        )?;
    }
    let mut boundaries = HashMap::<i64, HashSet<i64>>::new();
    for relation in &topology.face_loops {
        if !boundaries.contains_key(&relation.owner_ref) {
            ctx.reserve_map(&mut boundaries, 1, "index F3D aggregate boundary faces")?;
        }
        let edges = boundaries.entry(relation.owner_ref).or_default();
        for loop_slot in &relation.member_refs {
            ctx.charge_work(1, "walk F3D aggregate boundary loops")?;
            for coedge in loop_coedges.get(loop_slot).into_iter().flatten() {
                ctx.charge_work(1, "walk F3D aggregate boundary coedges")?;
                for edge in coedge_edges.get(coedge).into_iter().flatten() {
                    ctx.charge_work(1, "walk F3D aggregate boundary edges")?;
                    ctx.insert_hash_set(edges, *edge, "index F3D aggregate face edges")?;
                }
            }
        }
    }
    Ok(boundaries)
}
