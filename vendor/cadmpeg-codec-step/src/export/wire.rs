// SPDX-License-Identifier: Apache-2.0
//! Connected wire carriers, using vertex identities rather than coordinates.

use crate::writer::Ref;
use cadmpeg_ir::topology::Edge;
use std::collections::HashMap;

pub(super) fn connected_components(edges: &[(Ref, &Edge)]) -> Vec<Vec<Ref>> {
    fn root(parents: &mut [usize], mut index: usize) -> usize {
        while parents[index] != index {
            parents[index] = parents[parents[index]];
            index = parents[index];
        }
        index
    }
    let mut parents = (0..edges.len()).collect::<Vec<_>>();
    let mut ranks = vec![0u8; edges.len()];
    let mut vertices = HashMap::new();
    for (index, (_, edge)) in edges.iter().enumerate() {
        for vertex in [&edge.start, &edge.end] {
            if let Some(&previous) = vertices.get(vertex) {
                let left = root(&mut parents, index);
                let right = root(&mut parents, previous);
                if left != right {
                    match ranks[left].cmp(&ranks[right]) {
                        std::cmp::Ordering::Less => parents[left] = right,
                        std::cmp::Ordering::Greater => parents[right] = left,
                        std::cmp::Ordering::Equal => {
                            parents[right] = left;
                            ranks[left] += 1;
                        }
                    }
                }
            } else {
                vertices.insert(vertex, index);
            }
        }
    }
    let mut groups = HashMap::new();
    let mut components = Vec::<Vec<Ref>>::new();
    for (index, (emitted, _)) in edges.iter().enumerate() {
        let group = *groups.entry(root(&mut parents, index)).or_insert_with(|| {
            components.push(Vec::new());
            components.len() - 1
        });
        components[group].push(*emitted);
    }
    components
}

#[cfg(test)]
mod tests {
    use super::connected_components;
    use crate::writer::Ref;
    use cadmpeg_ir::{
        ids::{EdgeId, VertexId},
        topology::Edge,
    };

    #[test]
    fn components_follow_topological_vertices_including_bridges_branches_and_cycles() {
        let carrier = cadmpeg_ir::examples::unit_cube().expect("cube").model.edges[0]
            .carrier
            .clone();
        let edge = |id: usize, start: usize, end: usize| Edge {
            id: EdgeId::mint(format!("test:model:edge#{id}")).expect("edge id"),
            carrier: carrier.clone(),
            start: VertexId::mint(format!("test:model:vertex#{start}")).expect("vertex id"),
            end: VertexId::mint(format!("test:model:vertex#{end}")).expect("vertex id"),
            tolerance: None,
        };
        let edges = [
            edge(1, 1, 2),
            edge(2, 3, 4),
            edge(3, 2, 3),
            edge(4, 4, 1),
            edge(5, 2, 5),
            edge(6, 6, 7),
            edge(7, 7, 7),
        ];
        let emitted = edges
            .iter()
            .enumerate()
            .map(|(i, edge)| (Ref(cadmpeg_core::decode::u64_from_index(i) + 1), edge))
            .collect::<Vec<_>>();
        assert_eq!(
            connected_components(&emitted),
            vec![
                vec![Ref(1), Ref(2), Ref(3), Ref(4), Ref(5)],
                vec![Ref(6), Ref(7)]
            ]
        );
        assert!(connected_components(&[]).is_empty());
    }
}
