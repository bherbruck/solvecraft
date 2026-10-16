// SPDX-License-Identifier: Apache-2.0
//! Direct product ownership across representation and assembly boundaries.

use super::super::{named_parameter, topology::TopologyData, ValueExt};
use crate::parse::Exchange;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::BodyId;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct RepresentationOwnership<'a, 'ctx> {
    exchange: &'a Exchange,
    topology: &'a TopologyData,
    ctx: &'a DecodeContext<'ctx>,
    owners: BTreeMap<u64, BTreeSet<u64>>,
    children: BTreeMap<u64, BTreeSet<u64>>,
}

impl<'a, 'ctx> RepresentationOwnership<'a, 'ctx> {
    pub(super) fn build(
        exchange: &'a Exchange,
        topology: &'a TopologyData,
        shapes: &BTreeMap<u64, u64>,
        definitions: &BTreeMap<u64, u64>,
        ctx: &'a DecodeContext<'ctx>,
    ) -> Result<Self, CodecError> {
        let mut owners = BTreeMap::new();
        for (definition, representations) in
            super::definition_representations(exchange, shapes, ctx)?
        {
            if !definitions.contains_key(&definition) {
                continue;
            }
            for representation in representations {
                ctx.admit_btree_entry(&owners, &representation, "step representation owners")?;
                ctx.insert_btree_set(
                    owners.entry(representation).or_default(),
                    definition,
                    "step representation owners",
                )?;
            }
        }
        let mut children = BTreeMap::new();
        for (_, record) in exchange.entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
            let parent = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 3)
                .and_then(ValueExt::reference);
            let child = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 4)
                .and_then(ValueExt::reference);
            if let (Some(parent), Some(child)) = (parent, child) {
                ctx.admit_btree_entry(&children, &parent, "step ownership children")?;
                ctx.insert_btree_set(
                    children.entry(parent).or_default(),
                    child,
                    "step ownership children",
                )?;
            }
        }
        Ok(Self {
            exchange,
            topology,
            ctx,
            owners,
            children,
        })
    }

    pub(super) fn bodies(
        &self,
        definition: u64,
        representation: u64,
    ) -> Result<Vec<BodyId>, CodecError> {
        let ctx = self.ctx;
        let mut descendants = BTreeSet::new();
        let mut pending = ctx.alloc_filled(1, definition, "step ownership pending")?;
        while let Some(parent) = pending.pop() {
            ctx.charge_work(1, "step ownership walk")?;
            if !ctx.insert_btree_set(&mut descendants, parent, "step ownership descendants")? {
                continue;
            }
            for &child in self.children.get(&parent).into_iter().flatten() {
                ctx.push_vec(&mut pending, child, "step ownership pending")?;
            }
        }
        descendants.remove(&definition);
        let mut visited = BTreeSet::new();
        let mut bodies = BTreeSet::new();
        ctx.push_vec(&mut pending, representation, "step ownership pending")?;
        while let Some(current) = pending.pop() {
            ctx.charge_work(1, "step ownership walk")?;
            if !ctx.insert_btree_set(&mut visited, current, "step ownership visited")? {
                continue;
            }
            if let Some(direct) = self.topology.body_by_root.get(&current) {
                for body in direct {
                    if !bodies.contains(body) {
                        ctx.insert_btree_set(
                            &mut bodies,
                            body.try_clone_for_decode(ctx, "step owned body identity")?,
                            "step owned bodies",
                        )?;
                    }
                }
            }
            if let Some(items) = self
                .exchange
                .records()
                .get(&current)
                .and_then(super::super::topology::representation_item_values)
            {
                for item in items.iter().filter_map(ValueExt::reference) {
                    if self.topology.body_by_root.contains_key(&item) {
                        ctx.push_vec(&mut pending, item, "step ownership pending")?;
                    } else if let Some(mapped) =
                        self.exchange.records().get(&item).and_then(|record| {
                            super::super::topology::mapped_representation(record, self.exchange)
                        })
                    {
                        // A mapped child with an assembly usage is placed by that occurrence.
                        let placed_by_child = self.owners.get(&mapped).is_some_and(|owners| {
                            owners.iter().any(|owner| descendants.contains(owner))
                        });
                        if !placed_by_child {
                            ctx.push_vec(&mut pending, mapped, "step ownership pending")?;
                        }
                    }
                }
            }
            for &related in self
                .topology
                .shape_representation_relationships
                .get(&current)
                .into_iter()
                .flatten()
            {
                // Links within one definition may lead from a placement-only shape
                // to a geometric representation. A different definition owns its own bodies.
                let foreign = self
                    .owners
                    .get(&related)
                    .is_some_and(|owners| !owners.contains(&definition));
                if !foreign {
                    ctx.push_vec(&mut pending, related, "step ownership pending")?;
                }
            }
        }
        let mut output = ctx.collection_vec(bodies.len(), "step owned body output")?;
        output.extend(bodies);
        Ok(output)
    }
}
