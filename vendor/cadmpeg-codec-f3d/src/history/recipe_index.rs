// SPDX-License-Identifier: Apache-2.0
//! Index persistent recipe selectors once per immutable history snapshot.

use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

type TokenReferences<'a> = HashMap<&'a str, HashMap<i64, Vec<usize>>>;

pub(super) struct RecipeTopologyIndex<'a> {
    pub(super) topology: &'a AsmHistoricalTopology,
    tokens: TokenReferences<'a>,
    faces: Option<HashMap<i64, Vec<usize>>>,
}

impl<'a> RecipeTopologyIndex<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        topology: &'a AsmHistoricalTopology,
    ) -> Result<Self, CodecError> {
        let mut live_faces = HashSet::new();
        for face in &topology.faces {
            ctx.charge_work(1, "index F3D live recipe faces")?;
            ctx.insert_hash_set(&mut live_faces, *face, "index F3D live recipe faces")?;
        }
        let mut live_edges = HashSet::new();
        for edge in &topology.edges {
            ctx.charge_work(1, "index F3D live recipe edges")?;
            ctx.insert_hash_set(&mut live_edges, *edge, "index F3D live recipe edges")?;
        }
        let mut tokens = TokenReferences::new();
        for (index, tag) in topology.persistent_subentity_tags.iter().enumerate() {
            ctx.charge_work(1, "walk F3D recipe tags")?;
            let live = match tag.entity_kind {
                AsmHistoricalEntityKind::Face => live_faces.contains(&tag.entity_ref),
                AsmHistoricalEntityKind::Edge => live_edges.contains(&tag.entity_ref),
                _ => false,
            };
            if !live || tag.design_references.is_empty() {
                continue;
            }
            let token = tag.token.as_str();
            admit_key_work(ctx, token.len(), 2, 1, "index F3D recipe tokens")?;
            if !tokens.contains_key(token) {
                if tokens.len() == tokens.capacity() {
                    for key in tokens.keys() {
                        admit_key_work(ctx, key.len(), 1, 1, "rehash F3D recipe tokens")?;
                    }
                }
                ctx.reserve_map(&mut tokens, 1, "index F3D recipe tokens")?;
            }
            admit_key_work(ctx, token.len(), 1, 1, "index F3D recipe tokens")?;
            let references = tokens.entry(token).or_default();
            for reference in &tag.design_references {
                push_tag(
                    ctx,
                    references,
                    *reference,
                    index,
                    "index F3D recipe tag references",
                    "collect F3D recipe tag references",
                )?;
            }
        }
        Ok(Self {
            topology,
            tokens,
            faces: None,
        })
    }

    pub(super) fn matching_tags(
        &self,
        ctx: &DecodeContext<'_>,
        token: &str,
        reference: i64,
    ) -> Result<&[usize], CodecError> {
        admit_key_work(ctx, token.len(), 2, 18, "query F3D recipe tag references")?;
        Ok(self
            .tokens
            .get(token)
            .and_then(|references| references.get(&reference))
            .map_or(&[], Vec::as_slice))
    }

    pub(super) fn face_tags(
        &mut self,
        ctx: &DecodeContext<'_>,
        reference: i64,
    ) -> Result<&[usize], CodecError> {
        if self.faces.is_none() {
            let mut faces = HashMap::new();
            for references in self.tokens.values() {
                for (reference, tags) in references {
                    for tag_index in tags {
                        ctx.charge_work(1, "index F3D recipe face references")?;
                        if self.topology.persistent_subentity_tags[*tag_index].entity_kind
                            == AsmHistoricalEntityKind::Face
                        {
                            ctx.push_hash_group(
                                &mut faces,
                                *reference,
                                *tag_index,
                                "index F3D recipe face references",
                                "collect F3D recipe face references",
                            )?;
                        }
                    }
                }
            }
            for tags in faces.values_mut() {
                ctx.sort_unstable_by(tags, Ord::cmp, |_| 0, "sort F3D recipe face references")?;
            }
            self.faces = Some(faces);
        }
        ctx.charge_work(17, "query F3D recipe face references")?;
        Ok(self
            .faces
            .as_ref()
            .and_then(|faces| faces.get(&reference))
            .map_or(&[], Vec::as_slice))
    }
}

fn admit_key_work(
    ctx: &DecodeContext<'_>,
    count: usize,
    width: u64,
    extra: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    let work = u64_from_index(count)
        .checked_mul(width)
        .and_then(|work| work.checked_add(extra))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

fn push_tag(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<i64, Vec<usize>>,
    reference: i64,
    index: usize,
    index_operation: &'static str,
    value_operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(17, index_operation)?;
    if let Some(rows) = values.get(&reference) {
        // One tag can repeat its Design reference; the old membership query
        // selected that tag once, so retain one row in each reference group.
        if rows.last() == Some(&index) {
            return Ok(());
        }
        if rows.len() == rows.capacity() {
            admit_key_work(ctx, rows.len(), 8, 0, value_operation)?;
        }
    } else if values.len() == values.capacity() {
        admit_key_work(ctx, values.len(), 17, 0, index_operation)?;
    }
    // The grouped insertion performs two fixed-width map accesses.
    ctx.charge_work(34, index_operation)?;
    ctx.push_hash_group(values, reference, index, index_operation, value_operation)
}

#[derive(Default)]
pub(super) struct RecipeTopologyCache<'a> {
    entries: HashMap<*const AsmHistoricalTopology, RecipeTopologyIndex<'a>>,
}

impl<'a> RecipeTopologyCache<'a> {
    pub(super) fn get(
        &mut self,
        ctx: &DecodeContext<'_>,
        topology: &'a AsmHistoricalTopology,
    ) -> Result<&mut RecipeTopologyIndex<'a>, CodecError> {
        ctx.charge_work(1, "query F3D recipe topology cache")?;
        let key = std::ptr::from_ref(topology);
        if !self.entries.contains_key(&key) {
            let index = RecipeTopologyIndex::new(ctx, topology)?;
            ctx.reserve_map(&mut self.entries, 1, "cache F3D recipe topology")?;
            self.entries.insert(key, index);
        }
        self.entries
            .get_mut(&key)
            .ok_or_else(|| CodecError::malformed("recipe topology index was not built"))
    }
}

#[cfg(test)]
mod tests {
    use super::RecipeTopologyCache;
    use crate::history_records::{AsmHistoricalPersistentSubentityTag, AsmHistoricalTopology};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn recipe_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |entity_ref| AsmHistoricalTopology {
            faces: vec![entity_ref],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref,
                selector: 0,
                token: "rim".into(),
                design_references: vec![301, 301],
                ordinal: 0,
            }],
            ..AsmHistoricalTopology::default()
        };
        let mut first = topology(10);
        let mut dead = first.persistent_subentity_tags[0].clone();
        dead.entity_ref = 999;
        dead.token = "dead".into();
        first.persistent_subentity_tags.push(dead);
        let second = topology(20);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Each snapshot admits one live face, five selector-index slots,
        // and one cache slot. Queries admit no additional collection items.
        policy.limits.max_collection_items = 14;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            for (topology, entity) in [(&first, 10), (&second, 20)] {
                let index = cache.get(&ctx, topology).unwrap();
                assert_eq!(index.matching_tags(&ctx, "rim", 301).unwrap(), &[0]);
                assert_eq!(index.face_tags(&ctx, 301).unwrap(), &[0]);
                assert_eq!(
                    index.topology.persistent_subentity_tags[0].entity_ref,
                    entity
                );
                assert!(index.matching_tags(&ctx, "rim", 302).unwrap().is_empty());
                assert!(index.matching_tags(&ctx, "other", 301).unwrap().is_empty());
                assert!(index.matching_tags(&ctx, "dead", 301).unwrap().is_empty());
            }
        }
        ctx.finish_session().unwrap();
    }
    #[test]
    fn token_only_recipe_queries_do_not_admit_an_unused_face_index() {
        let topology = AsmHistoricalTopology {
            faces: vec![10],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref: 10,
                selector: 0,
                token: "rim".into(),
                design_references: vec![301],
                ordinal: 0,
            }],
            ..Default::default()
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One live face, token, reference, tag row, and cache entry.
        policy.limits.max_collection_items = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            assert_eq!(
                cache
                    .get(&ctx, &topology)
                    .unwrap()
                    .matching_tags(&ctx, "rim", 301)
                    .unwrap(),
                &[0]
            );
        }
        ctx.finish_session().unwrap();
    }
}
