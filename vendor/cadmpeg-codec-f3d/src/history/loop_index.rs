// SPDX-License-Identifier: Apache-2.0
//! Index historical loop incidence and point carriers without changing ambiguity gates.
use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(super) struct LoopIndex<'a> {
    pub(super) face_loops: HashMap<i64, Option<&'a [i64]>>,
    pub(super) loop_coedges: HashMap<i64, Option<&'a [i64]>>,
    pub(super) coedge_edges: HashMap<i64, Option<i64>>,
    pub(super) vertex_points: HashMap<i64, Option<i64>>,
    pub(super) positions: HashMap<i64, Option<cadmpeg_ir::math::Point3>>,
}

fn unique_values<K: Eq + std::hash::Hash, V>(
    ctx: &DecodeContext<'_>,
    rows: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, Option<V>>, CodecError> {
    let mut values = HashMap::new();
    for (key, value) in rows {
        ctx.charge_work(1, operation)?;
        if !values.contains_key(&key) {
            ctx.reserve_map(&mut values, 1, operation)?;
        }
        values
            .entry(key)
            .and_modify(|value| *value = None)
            .or_insert(Some(value));
    }
    Ok(values)
}

impl<'a> LoopIndex<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        topology: &'a AsmHistoricalTopology,
    ) -> Result<Self, CodecError> {
        let relations = |rows: &'a [AsmHistoricalRelation]| {
            unique_values(
                ctx,
                rows.iter()
                    .map(|row| (row.owner_ref, row.member_refs.as_slice())),
                "index F3D historical loop relations",
            )
        };
        Ok(Self {
            face_loops: relations(&topology.face_loops)?,
            loop_coedges: relations(&topology.loop_coedges)?,
            coedge_edges: unique_values(
                ctx,
                topology
                    .coedge_topology
                    .iter()
                    .map(|row| (row.coedge, row.edge)),
                "index F3D historical loop coedges",
            )?,
            vertex_points: unique_values(
                ctx,
                topology
                    .vertex_points
                    .iter()
                    .map(|row| (row.entity, row.carrier)),
                "index F3D historical loop points",
            )?,
            positions: unique_values(
                ctx,
                topology
                    .point_positions
                    .iter()
                    .map(|row| (row.point, row.position)),
                "index F3D historical loop positions",
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LoopIndex;
    use crate::history::topology_cache::{endpoint_index, TopologyQueryCache};
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalPoint, AsmHistoricalTopology,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    #[test]
    fn historical_loop_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |x| AsmHistoricalTopology {
            edge_vertices: vec![AsmHistoricalEdge {
                edge: 1,
                start_vertex: 2,
                end_vertex: 2,
            }],
            vertex_points: vec![AsmHistoricalCarrierBinding {
                entity: 2,
                carrier: 3,
            }],
            point_positions: vec![AsmHistoricalPoint {
                point: 3,
                position: cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
            }],
            ..Default::default()
        };
        let first = topology(1.0);
        let second = topology(2.0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 10;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cache = TopologyQueryCache::default();
        for _ in 0..1_000 {
            for (topology, x) in [(&first, 1.0), (&second, 2.0)] {
                let index = cache.loops.get(&ctx, topology, LoopIndex::new).unwrap();
                let endpoints = cache.endpoints.get(&ctx, topology, endpoint_index).unwrap();
                assert_eq!(endpoints[&1], Some([2, 2]));
                assert_eq!(index.vertex_points[&2], Some(3));
                assert_eq!(
                    index.positions[&3],
                    Some(cadmpeg_ir::math::Point3::new(x, 0.0, 0.0))
                );
            }
        }
        ctx.finish_session().unwrap();
    }
    #[test]
    fn historical_loop_indexes_reject_duplicate_carriers() {
        let topology = AsmHistoricalTopology {
            edge_vertices: vec![
                AsmHistoricalEdge {
                    edge: 1,
                    start_vertex: 2,
                    end_vertex: 2,
                },
                AsmHistoricalEdge {
                    edge: 1,
                    start_vertex: 2,
                    end_vertex: 2,
                },
            ],
            vertex_points: vec![
                AsmHistoricalCarrierBinding {
                    entity: 2,
                    carrier: 3,
                },
                AsmHistoricalCarrierBinding {
                    entity: 2,
                    carrier: 3,
                },
            ],
            point_positions: vec![
                AsmHistoricalPoint {
                    point: 3,
                    position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                },
                AsmHistoricalPoint {
                    point: 3,
                    position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                },
            ],
            ..Default::default()
        };
        let ctx = cadmpeg_test_support::service_decode_context();
        let index = LoopIndex::new(&ctx, &topology).unwrap();
        let endpoints = endpoint_index(&ctx, &topology).unwrap();
        assert_eq!(endpoints[&1], None);
        assert_eq!(index.vertex_points[&2], None);
        assert_eq!(index.positions[&3], None);
    }
}
