// SPDX-License-Identifier: Apache-2.0
//! Prove complete body membership from one immutable historical topology.
use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

fn occurrence_counts(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slots: &[i64],
) -> Result<HashMap<i64, usize>, cadmpeg_core::CodecError> {
    let mut counts = HashMap::new();
    for &slot in slots {
        decode.charge_work(1, "walk F3D complete body membership")?;
        if !counts.contains_key(&slot) {
            decode.reserve_map(&mut counts, 1, "index F3D complete body entity counts")?;
        }
        *counts.entry(slot).or_default() += 1;
    }
    Ok(counts)
}

struct RelationIndex<'a> {
    members_by_owner: HashMap<i64, Option<&'a [i64]>>,
    owner_by_member: HashMap<i64, Option<i64>>,
}

fn relation_index<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    relations: &'a [AsmHistoricalRelation],
) -> Result<RelationIndex<'a>, cadmpeg_core::CodecError> {
    let mut members_by_owner = HashMap::new();
    let mut owner_by_member = HashMap::new();
    for relation in relations {
        decode.charge_work(1, "walk F3D complete body membership")?;
        if !members_by_owner.contains_key(&relation.owner_ref) {
            decode.reserve_map(
                &mut members_by_owner,
                1,
                "index F3D complete body relation owners",
            )?;
        }
        members_by_owner
            .entry(relation.owner_ref)
            .and_modify(|members| *members = None)
            .or_insert(Some(relation.member_refs.as_slice()));
        for &member in &relation.member_refs {
            decode.charge_work(1, "walk F3D complete body membership")?;
            if !owner_by_member.contains_key(&member) {
                decode.reserve_map(
                    &mut owner_by_member,
                    1,
                    "index F3D complete body relation members",
                )?;
            }
            owner_by_member
                .entry(member)
                .and_modify(|owner| *owner = None)
                .or_insert(Some(relation.owner_ref));
        }
    }
    Ok(RelationIndex {
        members_by_owner,
        owner_by_member,
    })
}

pub(super) struct CompleteBodyIndex<'a> {
    body_counts: HashMap<i64, usize>,
    region_counts: HashMap<i64, usize>,
    shell_counts: HashMap<i64, usize>,
    face_counts: HashMap<i64, usize>,
    body_regions: RelationIndex<'a>,
    region_shells: RelationIndex<'a>,
    shell_faces: RelationIndex<'a>,
}
impl<'a> CompleteBodyIndex<'a> {
    pub(super) fn new(
        decode: &DecodeContext<'_>,
        topology: &'a AsmHistoricalTopology,
    ) -> Result<Self, CodecError> {
        let body_counts = occurrence_counts(decode, &topology.bodies)?;
        let region_counts = occurrence_counts(decode, &topology.regions)?;
        let shell_counts = occurrence_counts(decode, &topology.shells)?;
        let face_counts = occurrence_counts(decode, &topology.faces)?;
        let body_regions = relation_index(decode, &topology.body_regions)?;
        let region_shells = relation_index(decode, &topology.region_shells)?;
        let shell_faces = relation_index(decode, &topology.shell_faces)?;

        Ok(Self {
            body_counts,
            region_counts,
            shell_counts,
            face_counts,
            body_regions,
            region_shells,
            shell_faces,
        })
    }
    pub(super) fn faces(
        &self,
        decode: &DecodeContext<'_>,
        body: i64,
    ) -> Result<Option<Vec<i64>>, CodecError> {
        macro_rules! complete_some {
            ($value:expr) => {
                match $value {
                    Some(value) => value,
                    None => return Ok(None),
                }
            };
        }
        let Self {
            body_counts,
            region_counts,
            shell_counts,
            face_counts,
            body_regions,
            region_shells,
            shell_faces,
        } = self;
        if body_counts.get(&body).copied() != Some(1) {
            return Ok(None);
        }
        let regions = complete_some!(body_regions.members_by_owner.get(&body).copied().flatten());
        if regions.is_empty() {
            return Ok(None);
        }
        let mut seen_regions = HashSet::new();
        let mut seen_shells = HashSet::new();
        let mut seen_faces = HashSet::new();
        for &region in regions {
            decode.charge_work(1, "walk F3D complete body membership")?;
            if !decode.insert_hash_set(
                &mut seen_regions,
                region,
                "collect F3D complete body regions",
            )? || region_counts.get(&region).copied() != Some(1)
                || body_regions.owner_by_member.get(&region).copied().flatten() != Some(body)
            {
                return Ok(None);
            }
            let shells = complete_some!(region_shells
                .members_by_owner
                .get(&region)
                .copied()
                .flatten());
            if shells.is_empty() {
                return Ok(None);
            }
            for &shell in shells {
                decode.charge_work(1, "walk F3D complete body membership")?;
                if !decode.insert_hash_set(
                    &mut seen_shells,
                    shell,
                    "collect F3D complete body shells",
                )? || shell_counts.get(&shell).copied() != Some(1)
                    || region_shells.owner_by_member.get(&shell).copied().flatten() != Some(region)
                {
                    return Ok(None);
                }
                let faces =
                    complete_some!(shell_faces.members_by_owner.get(&shell).copied().flatten());
                if faces.is_empty() {
                    return Ok(None);
                }
                for &face in faces {
                    decode.charge_work(1, "walk F3D complete body membership")?;
                    if !decode.insert_hash_set(
                        &mut seen_faces,
                        face,
                        "collect F3D complete body faces",
                    )? || face_counts.get(&face).copied() != Some(1)
                        || shell_faces.owner_by_member.get(&face).copied().flatten() != Some(shell)
                    {
                        return Ok(None);
                    }
                }
            }
        }
        let mut faces = decode.collect_vec(seen_faces, "collect F3D complete body face slots")?;
        decode.sort_unstable_by(
            &mut faces,
            Ord::cmp,
            |_| 0,
            "sort F3D complete body face slots",
        )?;
        Ok((!faces.is_empty()).then_some(faces))
    }
}

#[cfg(test)]
mod tests {
    use super::CompleteBodyIndex;
    use crate::history::cache::SnapshotCache;
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    #[test]
    fn complete_body_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |face| AsmHistoricalTopology {
            bodies: vec![1],
            regions: vec![2],
            shells: vec![3],
            faces: vec![face],
            body_regions: vec![AsmHistoricalRelation {
                owner_ref: 1,
                member_refs: vec![2],
            }],
            region_shells: vec![AsmHistoricalRelation {
                owner_ref: 2,
                member_refs: vec![3],
            }],
            shell_faces: vec![AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![face],
            }],
            ..Default::default()
        };
        let first = topology(10);
        let second = topology(11);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Indexes admit ten map entries each. Each query admits three visited sets and one output slot.
        policy.limits.max_collection_items = 9_000;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cache = SnapshotCache::default();
        for _ in 0..1_000 {
            assert_eq!(
                cache
                    .get(&ctx, &first, CompleteBodyIndex::new)
                    .unwrap()
                    .faces(&ctx, 1)
                    .unwrap(),
                Some(vec![10])
            );
            assert_eq!(
                cache
                    .get(&ctx, &second, CompleteBodyIndex::new)
                    .unwrap()
                    .faces(&ctx, 1)
                    .unwrap(),
                Some(vec![11])
            );
        }
        ctx.finish_session().unwrap();
    }
}
