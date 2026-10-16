// SPDX-License-Identifier: Apache-2.0
//! Discover treatment supports once per immutable topology transition.
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

type TransitionKey = (*const AsmHistoricalTopology, *const AsmHistoricalTopology);
pub(super) struct TreatmentIndex {
    pub(super) result_boundaries: HashMap<i64, HashSet<i64>>,
    pub(super) preceding_boundaries: HashMap<i64, HashSet<i64>>,
    pub(super) supports: HashMap<i64, (i64, Vec<i64>)>,
    pub(super) radii: HashMap<i64, Option<f64>>,
}
impl TreatmentIndex {
    fn new(
        ctx: &DecodeContext<'_>,
        result: &AsmHistoricalTopology,
        preceding: &AsmHistoricalTopology,
    ) -> Result<Self, CodecError> {
        let result_boundaries = super::face_boundary_edge_index(ctx, result)?;
        let preceding_boundaries = super::face_boundary_edge_index(ctx, preceding)?;
        let rows = super::treatment_face_supports(
            ctx,
            &result.faces,
            result,
            preceding,
            &result_boundaries,
        )?;
        let supports = ctx.collect_hash_map(
            rows.into_iter()
                .map(|(face, carrier, supports)| (face, (carrier, supports))),
            "index F3D cached treatment supports",
        )?;
        let mut radii = HashMap::new();
        for radius in &result.surface_radii {
            ctx.charge_work(1, "index F3D treatment surface radii")?;
            if !radii.contains_key(&radius.surface) {
                ctx.reserve_map(&mut radii, 1, "index F3D treatment surface radii")?;
            }
            radii
                .entry(radius.surface)
                .and_modify(|value| *value = None)
                .or_insert(Some(radius.radius));
        }
        Ok(Self {
            result_boundaries,
            preceding_boundaries,
            supports,
            radii,
        })
    }
}
#[derive(Default)]
pub(super) struct TreatmentCache<'a> {
    entries: HashMap<TransitionKey, TreatmentIndex>,
    snapshots: PhantomData<&'a AsmHistoricalTopology>,
}
impl<'a> TreatmentCache<'a> {
    pub(super) fn get(
        &mut self,
        ctx: &DecodeContext<'_>,
        result: &'a AsmHistoricalTopology,
        preceding: &'a AsmHistoricalTopology,
    ) -> Result<&TreatmentIndex, CodecError> {
        ctx.charge_work(1, "query F3D treatment support cache")?;
        let key = (std::ptr::from_ref(result), std::ptr::from_ref(preceding));
        if !self.entries.contains_key(&key) {
            let index = TreatmentIndex::new(ctx, result, preceding)?;
            ctx.reserve_map(&mut self.entries, 1, "cache F3D treatment supports")?;
            self.entries.insert(key, index);
        }
        Ok(&self.entries[&key])
    }
}
