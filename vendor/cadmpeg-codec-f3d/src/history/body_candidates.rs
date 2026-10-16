// SPDX-License-Identifier: Apache-2.0
//! Resolve live and external body recipes with pass-local native identity indexes.
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::sketch_links::PersistentDesignLink;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::topology::{Body, Region, Shell};
use std::collections::{BTreeSet, HashMap};

struct ExternalIndex<'a> {
    body_by_face: HashMap<&'a FaceId, &'a BodyId>,
    metadata: HashMap<&'a BodyId, &'a Body>,
}
struct LinkIndex<'a> {
    recipes: HashMap<&'a str, Option<&'a ConstructionRecipe>>,
    selected: HashMap<(&'a str, i64), Option<&'a BodyId>>,
}

pub(super) struct BodyCandidates<'a> {
    bodies: &'a [Body],
    regions: &'a [Region],
    shells: &'a [Shell],
    recipes: &'a [ConstructionRecipe],
    links: &'a [PersistentDesignLink],
    external: Option<ExternalIndex<'a>>,
    linked: Option<LinkIndex<'a>>,
}
impl<'a> BodyCandidates<'a> {
    pub(super) fn new(
        bodies: &'a [Body],
        regions: &'a [Region],
        shells: &'a [Shell],
        recipes: &'a [ConstructionRecipe],
        links: &'a [PersistentDesignLink],
    ) -> Self {
        Self {
            bodies,
            regions,
            shells,
            recipes,
            links,
            external: None,
            linked: None,
        }
    }
    fn external_index(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<&ExternalIndex<'a>, CodecError> {
        if self.external.is_none() {
            let mut body_by_region = HashMap::new();
            for region in self.regions {
                ctx.charge_work(
                    u64_from_index(region.id.as_str().len()) + 1,
                    "index F3D external body regions",
                )?;
                ctx.insert_hash_map(
                    &mut body_by_region,
                    &region.id,
                    &region.body,
                    "index F3D external body regions",
                )?;
            }
            let mut body_by_face = HashMap::new();
            for shell in self.shells {
                ctx.charge_work(
                    u64_from_index(shell.region.as_str().len()) + 1,
                    "index F3D external body faces",
                )?;
                let Some(body) = body_by_region.get(&shell.region).copied() else {
                    continue;
                };
                for face in shell.faces() {
                    ctx.charge_work(
                        u64_from_index(face.as_str().len()) + 1,
                        "index F3D external body faces",
                    )?;
                    ctx.insert_hash_map(
                        &mut body_by_face,
                        face,
                        body,
                        "index F3D external body faces",
                    )?;
                }
            }
            let mut metadata = HashMap::new();
            for body in self.bodies {
                ctx.charge_work(
                    u64_from_index(body.id.as_str().len()) + 1,
                    "index F3D external body metadata",
                )?;
                ctx.insert_hash_map(
                    &mut metadata,
                    &body.id,
                    body,
                    "index F3D external body metadata",
                )?;
            }
            self.external = Some(ExternalIndex {
                body_by_face,
                metadata,
            });
        }
        self.external
            .as_ref()
            .ok_or_else(|| CodecError::malformed("external body index was not built"))
    }
    pub(super) fn face_body_candidates(
        &mut self,
        ctx: &DecodeContext<'_>,
        operand: &DesignBodyRecipeOperand,
        selected: &BodyId,
    ) -> Result<(bool, bool), CodecError> {
        let index = self.external_index(ctx)?;
        let mut any = false;
        let mut contains = false;
        for face in operand
            .references()
            .iter()
            .flat_map(|reference| &reference.candidate_faces)
        {
            ctx.charge_work(
                u64_from_index(face.as_str().len()) + 1,
                "resolve F3D body recipe face carrier",
            )?;
            if let Some(body) = index
                .body_by_face
                .get(face)
                .filter(|body| index.metadata.contains_key(*body))
            {
                any = true;
                contains |= *body == selected;
            }
        }
        Ok((any, contains))
    }
    pub(super) fn linked(
        &mut self,
        ctx: &DecodeContext<'_>,
        operand: &DesignBodyRecipeOperand,
    ) -> Result<Option<BodyId>, CodecError> {
        if self.recipes.is_empty() || self.links.is_empty() {
            return Ok(None);
        }
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            return Ok(None);
        };
        if self.linked.is_none() {
            let mut recipes = HashMap::new();
            for recipe in self.recipes {
                ctx.charge_work(1, "walk F3D body recipes")?;
                if recipe.kind != ConstructionRecipeKind::Body {
                    continue;
                }
                ctx.charge_work(
                    u64_from_index(recipe.id.len()) + 1,
                    "index F3D body recipe links",
                )?;
                if !recipes.contains_key(recipe.id.as_str()) {
                    ctx.reserve_map(&mut recipes, 1, "index F3D body recipe links")?;
                }
                recipes
                    .entry(recipe.id.as_str())
                    .and_modify(|row| *row = None)
                    .or_insert(Some(recipe));
            }
            for body in self.bodies {
                ctx.charge_work(
                    u64_from_index(body.id.as_str().len()) + 1,
                    "index F3D linked live bodies",
                )?;
            }
            let live = ctx.collect_hash_set(
                self.bodies.iter().map(|body| &body.id),
                "index F3D linked live bodies",
            )?;
            let mut latest = HashMap::<&BodyId, &PersistentDesignLink>::new();
            for link in self.links {
                ctx.charge_work(1, "walk F3D persistent body links")?;
                let cadmpeg_ir::attributes::AttributeTarget::Body(body) = &link.target else {
                    continue;
                };
                ctx.charge_work(
                    u64_from_index(body.as_str().len()) + 1,
                    "resolve F3D persistent body link",
                )?;
                if !latest.contains_key(body) {
                    ctx.reserve_map(&mut latest, 1, "index F3D latest body links")?;
                }
                latest
                    .entry(body)
                    .and_modify(|previous| {
                        if link.ordinal > previous.ordinal {
                            *previous = link;
                        }
                    })
                    .or_insert(link);
            }
            let mut selected = HashMap::new();
            for (body, link) in latest {
                if !live.contains(body) {
                    continue;
                }
                let key = (link.design_id.as_str(), link.design_reference);
                ctx.charge_work(
                    u64_from_index(key.0.len()) + 1,
                    "resolve F3D persistent body link",
                )?;
                if !selected.contains_key(&key) {
                    ctx.reserve_map(&mut selected, 1, "index F3D selected body links")?;
                }
                selected
                    .entry(key)
                    .and_modify(|previous| {
                        if *previous != Some(body) {
                            *previous = None;
                        }
                    })
                    .or_insert(Some(body));
            }
            self.linked = Some(LinkIndex { recipes, selected });
        }
        let Some(index) = self.linked.as_ref() else {
            return Ok(None);
        };
        ctx.charge_work(
            u64_from_index(operand.recipe_id.len()) + 1,
            "resolve F3D persistent body link",
        )?;
        let Some(recipe) = index
            .recipes
            .get(operand.recipe_id.as_str())
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        if crate::ids::native_stream(&recipe.id) != Some(stream) {
            return Ok(None);
        }
        let Some(design) = recipe.design.as_ref() else {
            return Ok(None);
        };
        let Some(selector) = design.selector else {
            return Ok(None);
        };
        ctx.charge_work(
            u64_from_index(design.id.value.len()) + 1,
            "resolve F3D persistent body link",
        )?;
        let Some(body) = index
            .selected
            .get(&(design.id.value.as_str(), i64::from(selector.value)))
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        Ok(Some(body.try_clone_for_decode(
            ctx,
            "copy F3D persistent body link identity",
        )?))
    }
    pub(super) fn direct(
        &mut self,
        ctx: &DecodeContext<'_>,
        operand: &DesignBodyRecipeOperand,
    ) -> Result<Option<BodyId>, CodecError> {
        if let Some(body) = self.linked(ctx, operand)? {
            let (any, contains) = self.face_body_candidates(ctx, operand, &body)?;
            return Ok((!any || contains).then_some(body));
        }
        self.external(ctx, operand, None)
    }
    pub(super) fn external(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
        current_history_source: Option<&str>,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
        let index = self.external_index(ctx)?;
        let body_by_face = &index.body_by_face;
        let body_metadata = &index.metadata;
        let current_prefix = current_history_source
            .map(|source| {
                ctx.format_retained(
                    format_args!("f3d:brep/{source}/"),
                    "retain F3D current history prefix",
                )
            })
            .transpose()?;
        let mut candidates: Option<BTreeSet<cadmpeg_ir::ids::BodyId>> = None;
        for reference in operand.references() {
            let mut reference_candidates = BTreeSet::new();
            for face in &reference.candidate_faces {
                ctx.charge_work(
                    u64_from_index(face.as_str().len()) + 1,
                    "query F3D external body faces",
                )?;
                let Some(body) = body_by_face.get(face).copied() else {
                    continue;
                };
                if current_prefix
                    .as_ref()
                    .is_some_and(|prefix| body.as_str().starts_with(prefix))
                {
                    continue;
                }
                if !reference_candidates.contains(body) {
                    let id = body.try_clone_for_decode(ctx, "copy F3D external body candidate")?;
                    ctx.insert_btree_set(
                        &mut reference_candidates,
                        id,
                        "collect F3D external body candidates",
                    )?;
                }
            }
            if let Some(candidates) = &mut candidates {
                if reference_candidates.is_empty() {
                    return Ok(None);
                }
                candidates.retain(|body| reference_candidates.contains(body));
            } else {
                candidates = Some(reference_candidates);
            }
        }
        let Some(mut candidates) = candidates else {
            return Ok(None);
        };
        let mut displayed = BTreeSet::new();
        for body in candidates.iter().filter(|body| {
            body_metadata
                .get(body)
                .is_some_and(|body| body.visible == Some(true))
        }) {
            let id = body.try_clone_for_decode(ctx, "copy F3D displayed external body")?;
            ctx.insert_btree_set(&mut displayed, id, "collect F3D displayed external bodies")?;
        }
        if !displayed.is_empty() {
            candidates = displayed;
        }
        if candidates.len() == 1 {
            Ok(candidates.into_iter().next())
        } else {
            Ok(None)
        }
    }
}
