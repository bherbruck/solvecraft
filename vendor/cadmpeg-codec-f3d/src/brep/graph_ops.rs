// SPDX-License-Identifier: Apache-2.0
//! Select dependencies and qualify identities in the typed BREP graph.

use super::Brep;
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use std::collections::{HashMap, HashSet};

pub(super) mod ordered;

use cadmpeg_ir::ids::comparison::{compare, equal};
use ordered::{admit_hash_key, contains_graph_id, insert_graph_id, select_links, select_rows};

type BrepAdjacency = HashMap<String, HashSet<String>>;

pub(super) fn insert_brep_adjacency(
    ctx: &DecodeContext<'_>,
    adjacency: &mut BrepAdjacency,
    source: &str,
    target: &str,
) -> Result<(), CodecError> {
    admit_hash_key(ctx, source, "index F3D BREP adjacency")?;
    if let Some(targets) = adjacency.get_mut(source) {
        if !contains_graph_id(ctx, targets, target, "find F3D BREP adjacent ID")? {
            let target = ctx.copy_retained_text(target, "copy F3D BREP adjacency target")?;
            insert_graph_id(ctx, targets, target, "collect F3D BREP adjacent IDs")?;
        }
        return Ok(());
    }
    let source = ctx.copy_retained_text(source, "copy F3D BREP adjacency source")?;
    if adjacency.len() == adjacency.capacity() {
        for key in adjacency.keys() {
            admit_hash_key(ctx, key, "rehash F3D BREP adjacency")?;
        }
    }
    ctx.reserve_map(adjacency, 1, "index F3D BREP adjacency")?;
    let mut targets = HashSet::new();
    let target = ctx.copy_retained_text(target, "copy F3D BREP adjacency target")?;
    insert_graph_id(ctx, &mut targets, target, "collect F3D BREP adjacent IDs")?;
    admit_hash_key(ctx, &source, "index F3D BREP adjacency")?;
    adjacency.insert(source, targets);
    Ok(())
}

fn add_dependencies<T: RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    adjacency: &mut BrepAdjacency,
    owner: &str,
    row: &T,
) -> Result<(), CodecError> {
    row.visit_identity_references(ctx, &mut |target| {
        if !equal(ctx, owner, target, "compare F3D dependency owner")? {
            insert_brep_adjacency(ctx, adjacency, owner, target)?;
        }
        Ok(())
    })
}

impl Brep {
    /// Retain the dependencies rooted at the selected native body selectors.
    pub(crate) fn retain_body_keys(
        &mut self,
        ctx: &DecodeContext<'_>,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), CodecError> {
        let reachable = ctx.with_scoped_storage("index F3D retained BREP graph", || {
            let mut native_bodies = HashSet::new();
            for native in &self.asm.body_native_keys {
                ctx.charge_work(1, "walk F3D native BREP bodies")?;
                let body =
                    ctx.copy_retained_text(native.body.as_str(), "copy F3D native BREP body")?;
                insert_graph_id(
                    ctx,
                    &mut native_bodies,
                    body,
                    "index F3D native BREP bodies",
                )?;
            }
            let mut reachable = HashSet::new();
            for body in self.body_selectors_for(ctx, selected_keys)?.into_keys() {
                ctx.charge_work(1, "walk F3D selected BREP roots")?;
                insert_graph_id(
                    ctx,
                    &mut reachable,
                    body.into_string(),
                    "collect F3D selected BREP roots",
                )?;
            }
            for body in &self.asm.bodies {
                ctx.charge_work(1, "walk F3D neutral BREP roots")?;
                if !contains_graph_id(
                    ctx,
                    &native_bodies,
                    body.id.as_str(),
                    "find F3D native BREP root",
                )? {
                    let id =
                        ctx.copy_retained_text(body.id.as_str(), "copy F3D neutral BREP root")?;
                    insert_graph_id(ctx, &mut reachable, id, "collect F3D neutral BREP roots")?;
                }
            }
            let mut adjacency = HashMap::new();
            macro_rules! dependencies {
                ($($field:ident),*) => {$(
                    for row in &self.asm.$field {
                        add_dependencies(ctx, &mut adjacency, row.id.as_str(), row)?;
                    }
                )*};
            }
            dependencies!(
                bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces,
                curves, pcurves
            );
            macro_rules! native_dependencies {
                ($($field:ident => $owner:ident),*) => {$(
                    for row in &self.asm.$field {
                        add_dependencies(ctx, &mut adjacency, row.$owner.as_str(), row)?;
                    }
                )*};
            }
            native_dependencies!(edge_continuities => edge, edge_ownerships => edge,
                vertex_ownerships => vertex, face_sidedness => face, face_native_keys => face,
                tolerant_coedge_parameters => coedge, tolerant_edge_tails => edge,
                tolerant_vertex_tails => vertex, mesh_surface_sentinels => surface,
                transform_hints => body, body_native_keys => body, wire_topologies => shell);
            for row in &self.asm.unknowns {
                add_dependencies(ctx, &mut adjacency, row.id().as_str(), row)?;
            }
            // Definitions are attached to carriers even when the carrier has a solved cache.
            // A tuple is an association, not an unowned row to retain indiscriminately.
            for (carrier, definition) in &self.asm.procedural_surfaces {
                add_dependencies(ctx, &mut adjacency, carrier.as_str(), definition)?;
            }
            for (carrier, definition) in &self.asm.procedural_curves {
                add_dependencies(ctx, &mut adjacency, carrier.as_str(), definition)?;
            }
            let mut pending = Vec::new();
            for id in &reachable {
                ctx.charge_work(1, "walk F3D BREP pending roots")?;
                ctx.push_vec(
                    &mut pending,
                    ctx.copy_retained_text(id.as_ref(), "copy F3D BREP pending root")?,
                    "collect F3D BREP pending roots",
                )?;
            }
            while let Some(id) = pending.pop() {
                ctx.charge_work(1, "walk F3D BREP pending IDs")?;
                admit_hash_key(ctx, &id, "find F3D BREP adjacent IDs")?;
                let adjacent_ids = adjacency.get(&id);
                for adjacent in adjacent_ids.into_iter().flatten() {
                    ctx.charge_work(1, "walk F3D BREP adjacent IDs")?;
                    let adjacent = adjacent.as_ref();
                    if !contains_graph_id(ctx, &reachable, adjacent, "find F3D reachable BREP ID")?
                    {
                        insert_graph_id(
                            ctx,
                            &mut reachable,
                            ctx.copy_retained_text(adjacent, "copy F3D reachable BREP ID")?,
                            "collect F3D reachable BREP IDs",
                        )?;
                        ctx.push_vec(
                            &mut pending,
                            ctx.copy_retained_text(adjacent, "copy F3D pending BREP ID")?,
                            "collect F3D pending BREP IDs",
                        )?;
                    }
                }
            }
            Ok::<_, CodecError>(reachable)
        })?;
        macro_rules! select {
            ($($field:ident => $owner:ident),*) => {$(
                self.asm.$field = select_rows(ctx, std::mem::take(&mut self.asm.$field), &reachable.0, |row| row.$owner.as_str())?;
            )*};
        }
        select!(bodies => id, regions => id, shells => id, faces => id,
            loops => id, coedges => id, edges => id, vertices => id,
            points => id, surfaces => id, curves => id, pcurves => id,
            edge_continuities => edge, edge_ownerships => edge,
            vertex_ownerships => vertex, face_sidedness => face,
            face_native_keys => face, tolerant_coedge_parameters => coedge,
            tolerant_edge_tails => edge, tolerant_vertex_tails => vertex,
            mesh_surface_sentinels => surface, transform_hints => body,
            body_native_keys => body, wire_topologies => shell);
        self.asm.unknowns = select_rows(
            ctx,
            std::mem::take(&mut self.asm.unknowns),
            &reachable.0,
            |row| row.id().as_str(),
        )?;
        self.asm.procedural_surfaces = select_rows(
            ctx,
            std::mem::take(&mut self.asm.procedural_surfaces),
            &reachable.0,
            |row| row.0.as_str(),
        )?;
        self.asm.procedural_curves = select_rows(
            ctx,
            std::mem::take(&mut self.asm.procedural_curves),
            &reachable.0,
            |row| row.0.as_str(),
        )?;
        self.asm.attributes = select_links(
            ctx,
            std::mem::take(&mut self.asm.attributes),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained attributes",
        )?;
        let mut annotations = Vec::new();
        for row in std::mem::take(&mut self.asm.annotation_records) {
            ctx.charge_work(1, "walk F3D retained annotations")?;
            if contains_graph_id(
                ctx,
                &reachable.0,
                &row.id,
                "select F3D retained annotations",
            )? {
                ctx.charge_work(
                    u64_from_index(std::mem::size_of_val(&row)),
                    "move F3D retained annotation",
                )?;
                ctx.push_vec(&mut annotations, row, "collect F3D retained annotations")?;
            }
        }
        self.asm.annotation_records = annotations;
        self.sketch_curve_links = select_links(
            ctx,
            std::mem::take(&mut self.sketch_curve_links),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained sketch links",
        )?;
        self.persistent_design_links = select_links(
            ctx,
            std::mem::take(&mut self.persistent_design_links),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained design links",
        )?;
        self.persistent_subentity_tags = select_links(
            ctx,
            std::mem::take(&mut self.persistent_subentity_tags),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained subentity tags",
        )?;
        self.creation_timestamps = select_links(
            ctx,
            std::mem::take(&mut self.creation_timestamps),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained timestamps",
        )?;
        Ok(())
    }

    /// Qualify identity fields while preserving ordinary text and numeric payloads.
    pub(crate) fn qualify_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: IdFormat,
        namespace: &str,
    ) -> Result<(), CodecError> {
        let prefix =
            ctx.format_retained(format_args!("{format}:"), "retain F3D BREP scheme prefix")?;
        let mut map =
            IdentityMap::new(ctx, "rewrite F3D qualified BREP fields", |source: &str| {
                let suffix = match source.get(..prefix.len()) {
                    Some(start)
                        if compare(ctx, start, &prefix, "match F3D BREP scheme prefix")?
                            .is_eq() =>
                    {
                        &source[prefix.len()..]
                    }
                    _ => source,
                };
                ctx.format_retained(
                    format_args!("{format}:brep/{namespace}/{suffix}"),
                    "copy F3D BREP remapped ID",
                )
            })?;
        let source = std::mem::take(self);
        let rewritten = source.rewrite_identities(ctx, &mut map);
        map.finish(ctx)?;
        *self = rewritten?;
        Ok(())
    }
}

impl RewriteIdentities for Brep {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk F3D BREP typed references")?;
        ctx.charge_work(1, "walk F3D BREP typed references")?;
        self.asm.visit_identity_references(ctx, visitor)?;
        self.sketch_curve_links
            .visit_identity_references(ctx, visitor)?;
        self.persistent_design_links
            .visit_identity_references(ctx, visitor)?;
        self.persistent_subentity_tags
            .visit_identity_references(ctx, visitor)?;
        self.creation_timestamps
            .visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite F3D BREP typed fields")?;
        ctx.charge_work(1, "rewrite F3D BREP typed fields")?;
        self.asm = self.asm.rewrite_identities(ctx, map)?;
        self.sketch_curve_links = self.sketch_curve_links.rewrite_identities(ctx, map)?;
        self.persistent_design_links = self.persistent_design_links.rewrite_identities(ctx, map)?;
        self.persistent_subentity_tags = self
            .persistent_subentity_tags
            .rewrite_identities(ctx, map)?;
        self.creation_timestamps = self.creation_timestamps.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::Brep;
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::ids::{BodyId, PointId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Body, BodyKind, Point};
    use std::collections::HashSet;

    fn graph() -> Brep {
        Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    kind: BodyKind::default(),
                    regions: vec![],
                    transform: None,
                    name: Some("f3d:brep:entity#1".into()),
                    color: None,
                    visible: None,
                }],
                points: vec![Point::new(
                    PointId::mint("f3d:brep:point#2").unwrap(),
                    FinitePoint3::new(Point3 {
                        x: f64::MAX,
                        y: f64::MIN_POSITIVE,
                        z: -0.0,
                    })
                    .unwrap(),
                    None,
                )],
                ..AsmBrep::default()
            },
            ..Brep::default()
        }
    }

    #[test]
    fn reverse_order_graph_insertion_uses_bounded_index_work() {
        use super::insert_brep_adjacency;
        use super::ordered::insert_graph_id;
        use std::collections::HashMap;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two thousand sorted-vector insertions shift 1,999,000 prior entries.
        // Indexed insertion admits key reads and geometric table growth instead.
        policy.limits.max_work_units = 300_000;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut adjacency = HashMap::new();
        let mut reachable = HashSet::new();
        for index in (0..2_000).rev() {
            let source = format!("s{index:04}");
            let target = format!("t{index:04}");
            insert_brep_adjacency(&ctx, &mut adjacency, &source, &target).unwrap();
            let source = ctx
                .copy_retained_text(&source, "copy synthetic graph ID")
                .unwrap();
            assert!(
                insert_graph_id(&ctx, &mut reachable, source, "index synthetic graph IDs").unwrap()
            );
        }
        assert_eq!(adjacency.len(), 2_000);
        assert_eq!(reachable.len(), 2_000);
        for index in 0..2_000 {
            let source = format!("s{index:04}");
            let target = format!("t{index:04}");
            assert_eq!(adjacency[&source], HashSet::from([target]));
            assert!(reachable.contains(&source));
        }
        ctx.finish_session().unwrap();
    }

    #[test]
    fn typed_brep_qualification_preserves_coordinate_bits_and_plain_identity_text() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Structural qualification admits backing map nodes within scoped storage.
        policy.limits.max_materialized_bytes = 1024 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut brep = graph();
        let original = brep.asm.points[0].position();
        brep.qualify_ids(&ctx, crate::ids::ID_FORMAT, "source")
            .unwrap();
        assert_eq!(
            brep.asm.bodies[0].id.as_str(),
            "f3d:brep/source/brep:entity#1"
        );
        assert_eq!(
            brep.asm.bodies[0].name.as_deref(),
            Some("f3d:brep:entity#1")
        );
        assert_eq!(
            brep.asm.points[0].id.as_str(),
            "f3d:brep/source/brep:point#2"
        );
        assert_eq!(
            brep.asm.points[0].position().x.to_bits(),
            original.x.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().y.to_bits(),
            original.y.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().z.to_bits(),
            original.z.to_bits()
        );
        let storage = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "BREP graph scratch released",
            )
            .unwrap();
        drop(storage);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn typed_brep_graph_paths_preserve_every_refusal_dimension() {
        for qualification in [false, true] {
            for dimension in [
                ResourceDimension::RetainedBytes,
                ResourceDimension::MaterializedBytes,
                ResourceDimension::CollectionItems,
                ResourceDimension::WorkUnits,
                ResourceDimension::RecursionDepth,
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = 0;
                    }
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                    ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                    _ => panic!("BREP dimensions"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut brep = graph();
                let error = if qualification {
                    brep.qualify_ids(&ctx, crate::ids::ID_FORMAT, "source")
                } else {
                    brep.retain_body_keys(&ctx, &HashSet::new())
                }
                .unwrap_err();
                let CodecError::ResourceLimit(first) = error else {
                    panic!("BREP graph must refuse");
                };
                assert_eq!(first.dimension, dimension);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
                );
            }
        }
    }

    #[test]
    fn carrier_selection_keeps_its_procedure_and_support_surface() {
        use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition, Surface};
        let mut model = cadmpeg_ir::examples::unit_cube().unwrap().model;
        let carrier = model.faces[0].surface.clone();
        let support = cadmpeg_ir::ids::SurfaceId::mint("f3d:brep:surface#support").unwrap();
        model.surfaces.push(Surface {
            id: support.clone(),
            geometry: model.surfaces[0].geometry.clone(),
            source_object: None,
        });
        let definition = ProceduralSurface::new(
            "f3d:brep:procedure#replica".try_into().unwrap(),
            ProceduralSurfaceDefinition::Replica {
                source: support.clone(),
                transform: cadmpeg_ir::transform::Transform::identity(),
            },
            None,
        );
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: model.bodies,
                regions: model.regions,
                shells: model.shells,
                faces: model.faces,
                loops: model.loops,
                coedges: model.coedges,
                edges: model.edges,
                vertices: model.vertices,
                points: model.points,
                surfaces: model.surfaces,
                curves: model.curves,
                pcurves: model.pcurves,
                procedural_surfaces: vec![(carrier, definition)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        brep.retain_body_keys(&ctx, &HashSet::new()).unwrap();
        assert_eq!(brep.asm.procedural_surfaces.len(), 1);
        assert!(brep
            .asm
            .surfaces
            .iter()
            .any(|surface| surface.id == support));
        ctx.finish_session().unwrap();
    }
}
