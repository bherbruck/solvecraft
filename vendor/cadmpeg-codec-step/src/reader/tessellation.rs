// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{ShadedVertex, Tessellation};
use cadmpeg_ir::transform::Transform;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut admitted_meshes = u64_from_index(ir.model.tessellations.len());
    let mut coordinate_map_bytes = ctx.reserve_scoped(0, "step_tessellation_coordinate_lists")?;
    let mut coordinates = BTreeMap::new();
    for (&id, record) in exchange.records() {
        if !has_entity(record, "COORDINATES_LIST") {
            continue;
        }
        let scale = geometry.units.length([id]).get();
        if let Some(vertices) = coordinate_rows(record, scale, ctx)? {
            coordinate_map_bytes.with_storage(|| {
                ctx.insert_btree_map(
                    &mut coordinates,
                    id,
                    vertices,
                    "step_tessellation_coordinate_lists",
                )
            })?;
        }
    }
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut reservations = AssociationReservations::new(ctx)?;
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut item_placements = BTreeMap::<u64, Vec<Transform>>::new();
    let mut unresolved_placements = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    let mut unresolved_containers = BTreeSet::new();
    let mut body_context_items = BTreeSet::new();
    for (&id, record) in exchange.records() {
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        let Some(items) = entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no structured items"),
                ctx,
            )?;
            continue;
        };
        let (item_ids, _container_item_bytes) = container_item_ids(items, kind, id, ctx)?;
        for &item in &item_ids {
            ctx.insert_scoped_btree_value(
                &mut reservations.declared,
                &mut declared_items,
                item,
                "step_tessellation_declared_items",
            )?;
        }
        let (candidates, _linked_body_bytes) = linked_bodies(record, kind, topology, ctx)?;
        if candidates.is_empty() {
            ctx.insert_scoped_btree_value(
                &mut reservations.unresolved_containers,
                &mut unresolved_containers,
                id,
                "step_tessellation_unresolved_containers",
            )?;
        }
        let mut body_candidate_bytes = {
            ctx.charge_collection_items(
                u64_from_index(candidates.len()),
                "step_tessellation_body_candidates",
            )?;
            ctx.reserve_scoped(0, "step_tessellation_body_candidates")
        }?;
        let body_candidates = body_candidate_bytes.with_storage(|| {
            collect_result_checked(
                ctx,
                candidates.len(),
                "step_tessellation_body_candidates",
                candidates.iter().map(|body| {
                    body.try_clone_for_decode(ctx, "step_tessellation_body_candidates")
                }),
            )
        })?;
        let mut associator = TessellationItemAssociator {
            bodies: &body_candidates,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::BodyItems,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        for item in item_ids {
            associator.visit(item, None)?;
        }
        ctx.insert_hash_set(&mut typed, id, "step_tessellation_claims")?;
    }
    let mut representation_storage =
        ctx.reserve_scoped(0, "step tessellation representation storage")?;
    let mut representation_cache = BTreeMap::new();
    let (product_representations, product_representation_bytes) =
        product_linked_representations(exchange, ctx)?;
    let (product_representation_items, product_item_bytes) =
        product_representation_items(exchange, &product_representations, ctx)?;
    for (&id, record) in exchange.records() {
        if !is_tessellated_shape_representation(record) {
            continue;
        }
        let Some((items, _representation_item_bytes)) = admitted_representation_items(record, ctx)?
        else {
            continue;
        };
        let bodies = representation_storage.with_storage(|| {
            super::topology::representation_bodies(
                id,
                exchange,
                topology,
                &mut representation_cache,
                &mut BTreeSet::new(),
                ctx,
            )
        })?;
        let product_linked = product_representations.contains(&id)
            || items
                .iter()
                .any(|item| product_representation_items.contains(item));
        if bodies.is_empty() && !product_linked {
            continue;
        }
        let mut associator = TessellationItemAssociator {
            bodies: &bodies,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::Placements,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        for item in items {
            associator.visit(item, None)?;
        }
    }
    drop(product_representation_items);
    drop(product_item_bytes);
    drop(product_representations);
    drop(product_representation_bytes);
    drop(representation_cache);
    for (&id, record) in exchange.records() {
        if !has_entity(record, "TESSELLATED_ANNOTATION_OCCURRENCE") {
            continue;
        }
        let Some(item) = tessellated_annotation_item(record) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("TESSELLATED_ANNOTATION_OCCURRENCE #{id} has no tessellated item"),
                ctx,
            )?;
            continue;
        };
        let mut associator = TessellationItemAssociator {
            bodies: &[],
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::DetachedAnnotation,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        associator.visit(item, None)?;
    }
    for id in unresolved_placements {
        push_loss(
            &mut losses,
            StepLossCode::TessellationPlacementUnresolved,
            format_args!("repositioned tessellated item #{id} has no valid AXIS2_PLACEMENT_3D; unresolved placement is not applied"),
            ctx,
        )?;
    }
    reservations.unresolved_placements =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_placements")?;
    for id in unresolved_containers {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        push_loss(
            &mut losses,
            StepLossCode::DecodeWarning,
            format_args!("{kind} #{id} has no decoded exact body link"),
            ctx,
        )?;
    }
    reservations.unresolved_containers =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_containers")?;
    for (&item, placements) in &item_placements {
        if !item_bodies.get(&item).is_some_and(BTreeSet::is_empty) {
            continue;
        }
        if distinct_placement(placements).is_none() {
            let distinct_count = distinct_placement_count(placements, ctx)?;
            push_loss(
                &mut losses,
                StepLossCode::TessellationPlacementAmbiguous,
                format_args!("tessellation item #{item} has {distinct_count} distinct repositioning placements; mesh retained in source coordinates"),
                ctx,
            )?;
        }
    }
    for (&item, bodies) in &item_bodies {
        if body_context_items.contains(&item) && bodies.len() != 1 {
            let detail = if bodies.is_empty() {
                "no decoded body"
            } else if bodies.len() > 1 {
                "multiple candidate bodies"
            } else {
                "an unresolved container association"
            };
            push_loss(
                &mut losses,
                StepLossCode::TessellationItemBodyUnresolved,
                format_args!("tessellation item #{item} has {detail}; mesh retained as detached"),
                ctx,
            )?;
        }
    }
    for (&id, record) in exchange.records() {
        let Some(entity) = TriangulatedEntity::of(record) else {
            continue;
        };
        let kind = entity.name();
        let base_kind = entity.base_name();
        let Some(coordinate_id) =
            inherited_parameter(record, base_kind, 0).and_then(ValueExt::reference)
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no COORDINATES_LIST reference"),
                ctx,
            )?;
            continue;
        };
        let Some((vertices, _coordinate_bytes)) = coordinates.get(&coordinate_id) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no resolved COORDINATES_LIST"),
                ctx,
            )?;
            continue;
        };
        let offset = entity.own_parameter_offset();
        let triangles = match entity {
            TriangulatedEntity::Face | TriangulatedEntity::SurfaceSet => {
                entity_parameter(record, kind, 1, offset)
                    .map(|value| triangle_rows(value, ctx))
                    .transpose()?
                    .flatten()
            }
            TriangulatedEntity::ComplexFace | TriangulatedEntity::ComplexSurfaceSet => {
                complex_triangles(
                    entity_parameter(record, kind, 1, offset),
                    entity_parameter(record, kind, 2, offset),
                    kind,
                    id,
                    ctx,
                )?
            }
        };
        let Some(AdmittedTriangles {
            triangles,
            bytes: triangle_bytes,
        }) = triangles.filter(|triangles| !triangles.triangles.is_empty())
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no triangle indices"),
                ctx,
            )?;
            continue;
        };
        let (pnindex, pnindex_bytes) = match entity_parameter(record, kind, 0, offset) {
            None | Some(Value::Omitted) => (Vec::new(), None),
            Some(value) => {
                let Some((indices, bytes)) = index_list(Some(value), ctx)? else {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!("{kind} #{id} has an invalid pnindex"),
                        ctx,
                    )?;
                    continue;
                };
                (indices, Some(bytes))
            }
        };
        let (
            mut local_vertices,
            local_triangles,
            addressing,
            _local_vertex_bytes,
            local_triangle_bytes,
            _coordinate_index_bytes,
        ) = if pnindex.is_empty() {
            if triangles.iter().flatten().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > vertices.len()
            }) {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based coordinate index"),
                    ctx,
                )?;
                continue;
            }
            let mut coordinate_index_bytes =
                ctx.reserve_scoped(0, "step_tessellation_coordinate_indices")?;
            let mut coordinate_indices = BTreeSet::new();
            for &index in triangles.iter().flatten() {
                if !coordinate_indices.contains(&index) {
                    coordinate_index_bytes.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut coordinate_indices,
                            index,
                            "step_tessellation_coordinate_indices",
                        )
                    })?;
                }
            }
            let mut local_index_bytes = ctx.reserve_scoped(0, "step_tessellation_local_index")?;
            let mut local_index = BTreeMap::new();
            for (local, global) in coordinate_indices.iter().enumerate() {
                let local = u32::try_from(local).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "step_tessellation_local_index_width",
                        u64::from(u32::MAX),
                        u64_from_index(local),
                    )
                })?;
                local_index_bytes.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut local_index,
                        *global,
                        local,
                        "step_tessellation_local_index",
                    )
                })?;
            }
            let mut local_vertex_bytes = {
                ctx.charge_collection_items(
                    u64_from_index(coordinate_indices.len()),
                    "step_tessellation_local_vertices",
                )?;
                ctx.reserve_scoped(0, "step_tessellation_local_vertices")
            }?;
            let local_vertices = local_vertex_bytes.with_storage(|| {
                collect_checked(
                    ctx,
                    coordinate_indices.len(),
                    "step_tessellation_local_vertices",
                    coordinate_indices
                        .iter()
                        .map(|index| vertices[cadmpeg_core::decode::index_from_u32(*index) - 1]),
                )
            })?;
            let mut local_triangle_bytes = {
                ctx.charge_collection_items(
                    u64_from_index(triangles.len()),
                    "step_tessellation_local_triangles",
                )?;
                ctx.reserve_scoped(0, "step_tessellation_local_triangles")
            }?;
            let local_triangles = local_triangle_bytes.with_storage(|| {
                collect_checked(
                    ctx,
                    triangles.len(),
                    "step_tessellation_local_triangles",
                    triangles
                        .iter()
                        .map(|triangle| triangle.map(|index| local_index[&index])),
                )
            })?;
            (
                local_vertices,
                local_triangles,
                CoordinateAddressing::TriangleIndices(coordinate_indices),
                local_vertex_bytes,
                local_triangle_bytes,
                Some(coordinate_index_bytes),
            )
        } else {
            if pnindex.iter().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > vertices.len()
            }) || triangles.iter().flatten().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > pnindex.len()
            }) {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based tessellation index"),
                    ctx,
                )?;
                continue;
            }
            let mut local_vertex_bytes = {
                ctx.charge_collection_items(
                    u64_from_index(pnindex.len()),
                    "step_tessellation_pn_vertices",
                )?;
                ctx.reserve_scoped(0, "step_tessellation_pn_vertices")
            }?;
            let mut local_triangle_bytes = {
                ctx.charge_collection_items(
                    u64_from_index(triangles.len()),
                    "step_tessellation_pn_triangles",
                )?;
                ctx.reserve_scoped(0, "step_tessellation_pn_triangles")
            }?;
            (
                local_vertex_bytes.with_storage(|| {
                    collect_checked(
                        ctx,
                        pnindex.len(),
                        "step_tessellation_pn_vertices",
                        pnindex.iter().map(|index| {
                            vertices[cadmpeg_core::decode::index_from_u32(*index) - 1]
                        }),
                    )
                })?,
                local_triangle_bytes.with_storage(|| {
                    collect_checked(
                        ctx,
                        triangles.len(),
                        "step_tessellation_pn_triangles",
                        triangles
                            .iter()
                            .map(|triangle| triangle.map(|index| index - 1)),
                    )
                })?,
                CoordinateAddressing::PnIndex,
                local_vertex_bytes,
                local_triangle_bytes,
                None,
            )
        };
        drop(triangles);
        drop(triangle_bytes);
        drop(pnindex);
        drop(pnindex_bytes);
        let (source_normals, _source_normal_bytes) = match inherited_parameter(record, base_kind, 2)
        {
            None | Some(Value::Omitted) => (Vec::new(), None),
            Some(value) => match normal_rows(Some(value), ctx)? {
                Some((normals, bytes)) => (normals, Some(bytes)),
                None => {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!("{kind} #{id} has invalid normal rows; normals omitted"),
                        ctx,
                    )?;
                    (Vec::new(), None)
                }
            },
        };
        // AP242 permits an empty normals aggregate; the IR represents both
        // that spelling and an omitted lane as an absent normal lane.
        let mut _replicated_normal_bytes = None;
        let mut projected_normal_bytes;
        let mut normals = match source_normals.len() {
            0 => None,
            1 => {
                let (replicated, storage) =
                    replicated_normals(local_vertices.len(), source_normals[0], ctx)?;
                _replicated_normal_bytes = Some(storage);
                Some(replicated)
            }
            count if count == local_vertices.len() => Some(source_normals),
            count => match &addressing {
                CoordinateAddressing::TriangleIndices(coordinate_indices)
                    if count == vertices.len() =>
                {
                    projected_normal_bytes = {
                        ctx.charge_collection_items(
                            u64_from_index(coordinate_indices.len()),
                            "step_tessellation_projected_normals",
                        )?;
                        ctx.reserve_scoped(0, "step_tessellation_projected_normals")
                    }?;
                    Some(projected_normal_bytes.with_storage(|| {
                        collect_checked(
                            ctx,
                            coordinate_indices.len(),
                            "step_tessellation_projected_normals",
                            coordinate_indices.iter().map(|index| {
                                source_normals[cadmpeg_core::decode::index_from_u32(*index) - 1]
                            }),
                        )
                    })?)
                }
                CoordinateAddressing::PnIndex | CoordinateAddressing::TriangleIndices(_) => {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!(
                            "{kind} #{id} carries {count} normals for {} coordinates",
                            local_vertices.len()
                        ),
                        ctx,
                    )?;
                    None
                }
            },
        };
        let mut placed_vertex_bytes;
        let mut placed_normal_bytes;
        if item_bodies.get(&id).is_some_and(BTreeSet::is_empty) {
            if let Some(placement) =
                distinct_placement(item_placements.get(&id).map_or(&[], Vec::as_slice))
            {
                placed_vertex_bytes = {
                    ctx.charge_collection_items(
                        u64_from_index(local_vertices.len()),
                        "step_tessellation_placed_vertices",
                    )?;
                    ctx.reserve_scoped(0, "step_tessellation_placed_vertices")
                }?;
                let placed_count = local_vertices.len();
                local_vertices = placed_vertex_bytes
                    .with_storage(|| {
                        collect_optional_checked(
                            ctx,
                            placed_count,
                            "step_tessellation_placed_vertices",
                            local_vertices
                                .into_iter()
                                .map(|vertex| placement.apply_point(vertex.get())),
                        )
                    })?
                    .ok_or_else(|| {
                        CodecError::malformed(format!(
                        "{kind} #{id} placed tessellation vertex contains a non-finite coordinate"
                    ))
                    })?;
                if let Some(source_normals) = normals.take() {
                    placed_normal_bytes = {
                        ctx.charge_collection_items(
                            u64_from_index(source_normals.len()),
                            "step_tessellation_placed_normals",
                        )?;
                        ctx.reserve_scoped(0, "step_tessellation_placed_normals")
                    }?;
                    let normal_count = source_normals.len();
                    match placed_normal_bytes.with_storage(|| {
                        collect_optional_checked(
                            ctx,
                            normal_count,
                            "step_tessellation_placed_normals",
                            source_normals.into_iter().map(|normal| {
                                placement
                                    .apply_normal(normal.get())
                                    .map(FiniteVector3::from)
                            }),
                        )
                    })? {
                        Some(transformed) => normals = Some(transformed),
                        None => {
                            push_loss(
                                &mut losses,
                                StepLossCode::DecodeWarning,
                                format_args!("{kind} #{id} normal placement could not produce finite unit normals; normals omitted"),
                                ctx,
                            )?;
                        }
                    }
                }
            }
        }
        if let Some(surface_step) = complex_triangulated_face_surface(record) {
            let (surface_id, _surface_id_bytes) = admitted_surface_id(surface_step, ctx)?;
            if let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id.as_str() == surface_id.as_str())
            {
                if surface.source_object.is_none() {
                    surface.source_object = Some(admitted_source_association(id, ctx)?);
                }
            }
        }
        let vertex_count = local_vertices.len();
        let triangle_count = local_triangles.len();
        let shaded = normals.is_some();
        let _shaded_row_bytes = if shaded {
            ctx.charge_collection_items(
                u64_from_index(vertex_count),
                "step_tessellation_shaded_rows",
            )?;
            Some(ctx.reserve_scoped(
                bytes_for::<ShadedVertex<FinitePoint3, FiniteVector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_shaded_rows",
                )?,
                "step_tessellation_shaded_rows",
            )?)
        } else {
            None
        };
        let rows = match cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
            local_vertices,
            local_triangles,
            normals,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                push_loss(
                    &mut losses,
                    StepLossCode::TessellationInvalidPayload,
                    format_args!("{kind} #{id}: {error}"),
                    ctx,
                )?;
                continue;
            }
        };
        ctx.charge_collection_items(
            u64_from_index(vertex_count),
            "step_tessellation_admitted_vertices",
        )?;
        let _admitted_vertex_bytes = ctx.reserve_scoped(
            if shaded {
                bytes_for::<ShadedVertex<FinitePoint3, Vector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_admitted_vertices",
                )?
            } else {
                bytes_for::<FinitePoint3>(vertex_count, ctx, "step_tessellation_admitted_vertices")?
            },
            "step_tessellation_admitted_vertices",
        )?;
        let _admitted_normal_bytes = if shaded {
            ctx.charge_collection_items(
                u64_from_index(vertex_count),
                "step_tessellation_admitted_normals",
            )?;
            Some(ctx.reserve_scoped(
                bytes_for::<ShadedVertex<FinitePoint3, FiniteVector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_admitted_normals",
                )?,
                "step_tessellation_admitted_normals",
            )?)
        } else {
            None
        };
        let _validation_triangle_bytes = ctx.reserve_scoped_collection::<[u32; 3]>(
            triangle_count,
            "step_tessellation_validation_triangles",
        )?;
        let retained_vertex_bytes = if shaded {
            bytes_for::<ShadedVertex<FinitePoint3, FiniteVector3>>(
                vertex_count,
                ctx,
                "step_tessellation_ir_mesh",
            )?
        } else {
            bytes_for::<FinitePoint3>(vertex_count, ctx, "step_tessellation_ir_mesh")?
        };
        ctx.charge_retained(retained_vertex_bytes, "step_tessellation_ir_mesh")?;
        let next_mesh_count = admitted_meshes.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_mesh_entity", u64::MAX - 1, u64::MAX)
        })?;
        let mut pending_meshes = admitted_meshes;
        ctx.admit_entities(
            next_mesh_count,
            &mut pending_meshes,
            "step_tessellation_mesh_entity",
        )?;
        let mesh = match Tessellation::from_parts(admitted_mesh_id(id, ctx)?, rows, Vec::new()) {
            Ok(mesh) => mesh,
            Err(error) => {
                push_loss(
                    &mut losses,
                    StepLossCode::TessellationInvalidPayload,
                    format_args!("{kind} #{id}: {error}"),
                    ctx,
                )?;
                continue;
            }
        };
        admitted_meshes = pending_meshes;
        local_triangle_bytes.commit()?;
        if !declared_items.contains(&id) {
            push_loss(
                &mut losses,
                StepLossCode::TessellationItemUndeclared,
                format_args!("tessellation item #{id} is not declared by an exact body container; mesh retained as detached"),
                ctx,
            )?;
        }

        ctx.reserve_vec(
            &mut ir.model.tessellations,
            1,
            "step_tessellation_mesh_list",
        )?;
        let body = (!item_bodies.get(&id).is_some_and(BTreeSet::is_empty))
            .then(|| item_bodies.get(&id))
            .flatten()
            .filter(|bodies| bodies.len() == 1)
            .and_then(|bodies| bodies.iter().next());
        let body = body
            .map(|body| body.try_clone_for_decode(ctx, "step_tessellation_mesh_body"))
            .transpose()?;
        let source_object = if !declared_items.contains(&id)
            || item_bodies.get(&id).is_some_and(BTreeSet::is_empty)
            || item_bodies.get(&id).is_none_or(|bodies| bodies.len() != 1)
        {
            Some(admitted_source_association(id, ctx)?)
        } else {
            None
        };

        ir.model
            .tessellations
            .push(mesh.with_body(body).with_source_object(source_object));
        ctx.insert_hash_set(&mut typed, id, "step_tessellation_claims")?;
        ctx.insert_hash_set(&mut typed, coordinate_id, "step_tessellation_claims")?;
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in exchange.records() {
            if has_entity(record, "TESSELLATED_SHAPE_REPRESENTATION")
                || has_entity(record, "TESSELLATED_SOLID")
                || has_entity(record, "TESSELLATED_SHELL")
            {
                ctx.insert_hash_set(&mut typed, id, "step_tessellation_claims")?;
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

/// How a tessellated item addresses the coordinate list it shares.
///
/// A STEP tessellated item states either a PNINDEX list, which names the
/// coordinates it uses, or no PNINDEX at all, in which case its triangle
/// indices name them directly. The two are one statement of the record, so
/// they are one value: the coordinate set exists exactly when the record
/// states no PNINDEX.
enum CoordinateAddressing {
    /// The record states a PNINDEX list.
    PnIndex,
    /// The record states no PNINDEX; these triangle indices name the
    /// coordinates it uses, in ascending order.
    TriangleIndices(BTreeSet<u32>),
}

#[derive(Debug)]
struct AdmittedTriangles<'a> {
    triangles: Vec<[u32; 3]>,
    bytes: ScopedReservation<'a>,
}

fn complex_triangulated_face_surface(record: &RawRecord) -> Option<u64> {
    has_entity(record, "COMPLEX_TRIANGULATED_FACE")
        .then(|| inherited_parameter(record, "TESSELLATED_FACE", 3))
        .flatten()
        .and_then(ValueExt::reference)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssociationMode {
    BodyItems,
    Placements,
    DetachedAnnotation,
}

struct AssociationReservations<'a> {
    active: ScopedReservation<'a>,
    declared: ScopedReservation<'a>,
    unresolved_containers: ScopedReservation<'a>,
    unresolved_placements: ScopedReservation<'a>,
    body_context: ScopedReservation<'a>,
    item_bodies: ScopedReservation<'a>,
    placements: ScopedReservation<'a>,
}

impl<'a> AssociationReservations<'a> {
    fn new(ctx: &'a DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            active: ctx.reserve_scoped(0, "step_tessellation_active_items")?,
            declared: ctx.reserve_scoped(0, "step_tessellation_declared_items")?,
            unresolved_containers: ctx
                .reserve_scoped(0, "step_tessellation_unresolved_containers")?,
            unresolved_placements: ctx
                .reserve_scoped(0, "step_tessellation_unresolved_placements")?,
            body_context: ctx.reserve_scoped(0, "step_tessellation_body_context_items")?,
            item_bodies: ctx.reserve_scoped(0, "step_tessellation_item_bodies")?,
            placements: ctx.reserve_scoped(0, "step_tessellation_item_placements")?,
        })
    }
}

struct TessellationItemAssociator<'a, 'ctx, 'arena> {
    bodies: &'a [BodyId],
    exchange: &'a Exchange,
    item_bodies: &'a mut BTreeMap<u64, BTreeSet<BodyId>>,
    declared_items: &'a mut BTreeSet<u64>,
    unresolved_containers: &'a mut BTreeSet<u64>,
    typed: &'a mut HashSet<u64>,
    geometry: &'a GeometryData,
    placements: &'a mut BTreeMap<u64, Vec<Transform>>,
    unresolved_placements: &'a mut BTreeSet<u64>,
    body_context_items: &'a mut BTreeSet<u64>,
    mode: AssociationMode,
    active: BTreeSet<u64>,
    reservations: &'a mut AssociationReservations<'ctx>,
    ctx: &'ctx DecodeContext<'arena>,
}

impl TessellationItemAssociator<'_, '_, '_> {
    fn visit(&mut self, id: u64, inherited_placement: Option<Transform>) -> Result<(), CodecError> {
        let _depth_guard = self.ctx.enter_nested("step_tessellation_association")?;
        if self.active.contains(&id) {
            return Ok(());
        }
        self.reservations.active.with_storage(|| {
            self.ctx
                .insert_btree_set(&mut self.active, id, "step_tessellation_active_items")
        })?;
        let Some(record) = self.exchange.records().get(&id) else {
            self.active.remove(&id);
            return Ok(());
        };
        let local_placement = if has_entity(record, "REPOSITIONED_TESSELLATED_ITEM") {
            let placement = repositioned_placement(record, self.geometry);
            if placement.is_none() {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.unresolved_placements,
                    self.unresolved_placements,
                    id,
                    "step_tessellation_unresolved_placements",
                )?;
            }
            placement
        } else {
            None
        };
        let placement = match (inherited_placement, local_placement) {
            (Some(parent), Some(local)) => Some(parent.compose(local).map_err(|error| {
                CodecError::malformed(format_args!(
                    "invalid STEP tessellation placement #{id}: {error}"
                ))
            })?),
            (parent, None) => parent,
            (None, local) => local,
        };
        if entity_kind(
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        )
        .is_some()
        {
            if self.mode != AssociationMode::DetachedAnnotation {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.body_context,
                    self.body_context_items,
                    id,
                    "step_tessellation_body_context_items",
                )?;
            }
            self.ctx.insert_scoped_btree_value(
                &mut self.reservations.declared,
                self.declared_items,
                id,
                "step_tessellation_declared_items",
            )?;
            associate_bodies(
                self.item_bodies,
                id,
                self.bodies,
                self.ctx,
                &mut self.reservations.item_bodies,
            )?;
            if let Some(placement) = placement {
                push_placement(
                    self.placements,
                    id,
                    placement,
                    self.ctx,
                    &mut self.reservations.placements,
                )?;
            }
        } else if let Some(kind) = entity_kind(
            record,
            &[
                "TESSELLATED_SOLID",
                "TESSELLATED_SHELL",
                "TESSELLATED_GEOMETRIC_SET",
            ],
        ) {
            if self.mode == AssociationMode::BodyItems {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.declared,
                    self.declared_items,
                    id,
                    "step_tessellation_declared_items",
                )?;
                associate_bodies(
                    self.item_bodies,
                    id,
                    self.bodies,
                    self.ctx,
                    &mut self.reservations.item_bodies,
                )?;
            }
            if self.mode != AssociationMode::DetachedAnnotation {
                self.ctx
                    .insert_hash_set(self.typed, id, "step_tessellation_claims")?;
            }
            if !self.bodies.is_empty() && matches!(kind, "TESSELLATED_SOLID" | "TESSELLATED_SHELL")
            {
                self.unresolved_containers.remove(&id);
            }
            let (item_ids, _container_item_bytes) =
                match entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) {
                    Some(items) => container_item_ids(items, kind, id, self.ctx)?,
                    None => (
                        Vec::new(),
                        self.ctx
                            .reserve_scoped(0, "step_tessellation_container_items")?,
                    ),
                };
            for item in item_ids {
                self.visit(item, placement)?;
            }
        } else if self.mode == AssociationMode::BodyItems {
            self.ctx.insert_scoped_btree_value(
                &mut self.reservations.declared,
                self.declared_items,
                id,
                "step_tessellation_declared_items",
            )?;
            associate_bodies(
                self.item_bodies,
                id,
                self.bodies,
                self.ctx,
                &mut self.reservations.item_bodies,
            )?;
        }
        self.active.remove(&id);
        Ok(())
    }
}

fn distinct_placement_count(
    placements: &[Transform],
    ctx: &DecodeContext<'_>,
) -> Result<usize, CodecError> {
    let count = u64_from_index(placements.len());
    // Every prior-placement comparison can read the complete transform.
    let work = count
        .checked_mul(count)
        .and_then(|comparisons| {
            comparisons.checked_mul(u64_from_index(std::mem::size_of::<Transform>()))
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_distinct_placements", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(work, "step_tessellation_distinct_placements")?;
    Ok(placements
        .iter()
        .enumerate()
        .filter(|(index, placement)| !placements[..*index].contains(placement))
        .count())
}

fn distinct_placement(placements: &[Transform]) -> Option<Transform> {
    let first = *placements.first()?;
    placements
        .iter()
        .all(|placement| *placement == first)
        .then_some(first)
}

fn repositioned_placement(record: &RawRecord, geometry: &GeometryData) -> Option<Transform> {
    let placement_id = entity_parameter(record, "REPOSITIONED_TESSELLATED_ITEM", 0, 1)
        .and_then(ValueExt::reference)?;
    geometry
        .placements
        .get(&placement_id)
        .copied()
        .and_then(super::geometry::placement_transform)
}

fn tessellated_annotation_item(record: &RawRecord) -> Option<u64> {
    for name in ["TESSELLATED_ANNOTATION_OCCURRENCE", "STYLED_ITEM"] {
        let Some(partial) = record.partials.iter().find(|partial| partial.name == name) else {
            continue;
        };
        if let Some(item) = partial
            .parameters
            .iter()
            .rev()
            .find_map(ValueExt::reference)
        {
            return Some(item);
        }
    }
    None
}

fn associate_bodies(
    associations: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    item: u64,
    bodies: &[BodyId],
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !associations.contains_key(&item) {
        bytes.with_storage(|| {
            ctx.admit_btree_entry(associations, &item, "step_tessellation_item_body_entries")
        })?;
    }
    let associated = associations.entry(item).or_default();
    for body in bodies {
        if !associated.contains(body) {
            let copy = bytes.with_storage(|| {
                body.try_clone_for_decode(ctx, "step_tessellation_item_body_links")
            })?;
            bytes.with_storage(|| {
                ctx.insert_btree_set(associated, copy, "step_tessellation_item_body_links")
            })?;
        }
    }
    Ok(())
}

fn push_placement(
    placements: &mut BTreeMap<u64, Vec<Transform>>,
    item: u64,
    placement: Transform,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !placements.contains_key(&item) {
        bytes.with_storage(|| {
            ctx.admit_btree_entry(placements, &item, "step_tessellation_placement_entries")
        })?;
    }

    let values = placements.entry(item).or_default();

    ctx.reserve_scoped_vec(bytes, values, 1, "step_tessellation_placements")?;
    values.push(placement);
    Ok(())
}

fn insert_relationship(
    relationships: &mut BTreeMap<u64, BTreeSet<u64>>,
    source: u64,
    target: u64,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !relationships.contains_key(&source) {
        bytes.with_storage(|| {
            ctx.admit_btree_entry(
                relationships,
                &source,
                "step_tessellation_relationship_nodes",
            )
        })?;
    }
    let targets = relationships.entry(source).or_default();
    ctx.insert_scoped_btree_value(
        bytes,
        targets,
        target,
        "step_tessellation_relationship_edges",
    )?;
    Ok(())
}

fn product_linked_representations<'a>(
    exchange: &Exchange,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<u64>, ScopedReservation<'a>), CodecError> {
    let mut product_shape_definitions = BTreeSet::new();
    let mut definition_bytes = ctx.reserve_scoped(0, "step_tessellation_product_definitions")?;
    for (id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "PRODUCT_DEFINITION_SHAPE")
        {
            ctx.insert_scoped_btree_value(
                &mut definition_bytes,
                &mut product_shape_definitions,
                id,
                "step_tessellation_product_definitions",
            )?;
        }
    }
    let mut linked = BTreeSet::new();
    let mut linked_bytes = ctx.reserve_scoped(0, "step_tessellation_product_representations")?;
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let representation = (|| {
            let partial = record
                .partials
                .iter()
                .find(|partial| partial.name == "SHAPE_DEFINITION_REPRESENTATION")?;
            let definition = partial.parameters.first().and_then(ValueExt::reference)?;
            product_shape_definitions
                .contains(&definition)
                .then(|| partial.parameters.get(1).and_then(ValueExt::reference))?
        })();
        if let Some(representation) = representation {
            ctx.insert_scoped_btree_value(
                &mut linked_bytes,
                &mut linked,
                representation,
                "step_tessellation_product_representations",
            )?;
        }
    }
    if linked.is_empty() {
        return Ok((linked, linked_bytes));
    }
    let mut relationships = BTreeMap::<u64, BTreeSet<u64>>::new();
    let mut relationship_bytes = ctx.reserve_scoped(0, "step_tessellation_relationship_edges")?;
    for record in exchange.records().values() {
        let Some(shape_relationship) = record
            .partials
            .iter()
            .find(|partial| partial.name == "SHAPE_REPRESENTATION_RELATIONSHIP")
        else {
            continue;
        };
        let (left, right) = {
            let mut references = shape_relationship
                .parameters
                .iter()
                .filter_map(ValueExt::reference);
            match (references.next(), references.next()) {
                (Some(left), Some(right)) => (left, right),
                _ => {
                    let Some(base) = record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "REPRESENTATION_RELATIONSHIP")
                    else {
                        continue;
                    };
                    let mut references = base.parameters.iter().filter_map(ValueExt::reference);
                    let (Some(left), Some(right)) = (references.next(), references.next()) else {
                        continue;
                    };
                    (left, right)
                }
            }
        };
        insert_relationship(
            &mut relationships,
            left,
            right,
            ctx,
            &mut relationship_bytes,
        )?;
        insert_relationship(
            &mut relationships,
            right,
            left,
            ctx,
            &mut relationship_bytes,
        )?;
    }
    let mut pending_bytes = {
        ctx.charge_collection_items(
            u64_from_index(linked.len()),
            "step_tessellation_product_pending",
        )?;
        ctx.reserve_scoped(0, "step_tessellation_product_pending")
    }?;
    let mut pending = Vec::new();

    pending_bytes.with_storage(|| {
        ctx.reserve_capacity(
            &mut pending,
            linked.len(),
            "step_tessellation_product_pending",
        )
    })?;
    pending.extend(linked.iter().copied());
    while let Some(representation) = pending.pop() {
        for &related in relationships.get(&representation).into_iter().flatten() {
            if ctx.insert_scoped_btree_value(
                &mut linked_bytes,
                &mut linked,
                related,
                "step_tessellation_product_representations",
            )? {
                ctx.reserve_scoped_vec(
                    &mut pending_bytes,
                    &mut pending,
                    1,
                    "step_tessellation_product_pending",
                )?;
                pending.push(related);
            }
        }
    }
    Ok((linked, linked_bytes))
}

fn product_representation_items<'a>(
    exchange: &Exchange,
    representations: &BTreeSet<u64>,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<u64>, ScopedReservation<'a>), CodecError> {
    let mut items = BTreeSet::new();
    let mut bytes = ctx.reserve_scoped(0, "step_tessellation_product_items")?;
    for record in representations
        .iter()
        .filter_map(|id| exchange.records().get(id))
    {
        if let Some((representation_items, _representation_item_bytes)) =
            admitted_representation_items(record, ctx)?
        {
            for item in representation_items {
                ctx.insert_scoped_btree_value(
                    &mut bytes,
                    &mut items,
                    item,
                    "step_tessellation_product_items",
                )?;
            }
        }
    }
    Ok((items, bytes))
}

fn admitted_representation_items<'a>(
    record: &RawRecord,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<u64>, ScopedReservation<'a>)>, CodecError> {
    let Some(values) = super::representation::item_values(record) else {
        return Ok(None);
    };
    let count = values
        .iter()
        .filter(|value| value.reference().is_some())
        .count();
    let mut bytes = {
        ctx.charge_collection_items(
            u64_from_index(count),
            "step_tessellation_representation_items",
        )?;
        ctx.reserve_scoped(0, "step_tessellation_representation_items")
    }?;
    let items = bytes.with_storage(|| {
        collect_checked(
            ctx,
            count,
            "step_tessellation_representation_items",
            values.iter().filter_map(ValueExt::reference),
        )
    })?;
    Ok(Some((items, bytes)))
}

fn linked_bodies<'a>(
    record: &RawRecord,
    kind: &str,
    topology: &TopologyData,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<BodyId>, ScopedReservation<'a>), CodecError> {
    let Some(link) = entity_parameter(record, kind, 1, 1).and_then(ValueExt::reference) else {
        return Ok((
            BTreeSet::new(),
            ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?,
        ));
    };
    match kind {
        "TESSELLATED_SOLID" => {
            let bodies = topology.body_by_root.get(&link);
            let mut bytes = ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?;
            let mut linked = BTreeSet::new();
            for body in bodies.into_iter().flatten() {
                bytes.with_storage(|| {
                    let body = body.try_clone_for_decode(ctx, "step_tessellation_linked_bodies")?;
                    ctx.insert_btree_set(&mut linked, body, "step_tessellation_linked_bodies")
                })?;
            }
            Ok((linked, bytes))
        }
        "TESSELLATED_SHELL" => {
            let bodies = topology.body_by_shell.get(&link);
            let mut bytes = ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?;
            let mut linked = BTreeSet::new();
            if let Some(bodies) = bodies {
                for body in bodies {
                    bytes.with_storage(|| {
                        let body =
                            body.try_clone_for_decode(ctx, "step_tessellation_linked_bodies")?;
                        ctx.insert_btree_set(&mut linked, body, "step_tessellation_linked_bodies")
                    })?;
                }
            }
            Ok((linked, bytes))
        }
        _ => Ok((
            BTreeSet::new(),
            ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?,
        )),
    }
}

fn index_list<'a>(
    value: Option<&Value>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<u32>, ScopedReservation<'a>)>, CodecError> {
    let Some(values) = value.and_then(ValueExt::list) else {
        return Ok(None);
    };
    let mut bytes = {
        ctx.charge_collection_items(u64_from_index(values.len()), "step_tessellation_pnindex")?;
        ctx.reserve_scoped(0, "step_tessellation_pnindex")
    }?;
    Ok(bytes
        .with_storage(|| {
            collect_optional_checked(
                ctx,
                values.len(),
                "step_tessellation_pnindex",
                values
                    .iter()
                    .map(|value| u32::try_from(value.integer()?).ok()),
            )
        })?
        .map(|indices| (indices, bytes)))
}

fn container_item_ids<'a>(
    items: &[Value],
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<u64>, ScopedReservation<'a>), CodecError> {
    let mut bytes = {
        ctx.charge_collection_items(
            u64_from_index(items.len()),
            "step_tessellation_container_items",
        )?;
        ctx.reserve_scoped(0, "step_tessellation_container_items")
    }?;
    let ids = bytes.with_storage(|| {
        collect_result_checked(
            ctx,
            items.len(),
            "step_tessellation_container_items",
            items.iter().enumerate().map(|(index, item)| {
                item.reference().ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "{kind} #{id} item {} is not a reference",
                        index + 1
                    ))
                })
            }),
        )
    })?;
    Ok((ids, bytes))
}

fn has_entity(record: &RawRecord, name: &str) -> bool {
    entity_kind(record, &[name]).is_some()
}

fn is_tessellated_shape_representation(record: &RawRecord) -> bool {
    entity_kind(
        record,
        &[
            "TESSELLATED_SHAPE_REPRESENTATION",
            "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS",
        ],
    )
    .is_some()
}

fn entity_kind<'a>(record: &'a RawRecord, names: &[&str]) -> Option<&'a str> {
    record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| *name == partial.name))
        .map(|partial| partial.name.as_str())
}

fn entity_parameter<'a>(
    record: &'a RawRecord,
    entity: &str,
    index: usize,
    simple_offset: usize,
) -> Option<&'a Value> {
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == entity)?;
    let offset = if record.partials.len() == 1 {
        simple_offset
    } else {
        0
    };
    partial.parameters.get(index + offset)
}

/// One triangulated tessellation entity, parsed from the record's partial names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriangulatedEntity {
    Face,
    ComplexFace,
    SurfaceSet,
    ComplexSurfaceSet,
}

impl TriangulatedEntity {
    fn of(record: &RawRecord) -> Option<Self> {
        record.partials.iter().find_map(|partial| {
            Some(match partial.name.as_str() {
                "TRIANGULATED_FACE" => Self::Face,
                "COMPLEX_TRIANGULATED_FACE" => Self::ComplexFace,
                "TRIANGULATED_SURFACE_SET" => Self::SurfaceSet,
                "COMPLEX_TRIANGULATED_SURFACE_SET" => Self::ComplexSurfaceSet,
                _ => return None,
            })
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Face => "TRIANGULATED_FACE",
            Self::ComplexFace => "COMPLEX_TRIANGULATED_FACE",
            Self::SurfaceSet => "TRIANGULATED_SURFACE_SET",
            Self::ComplexSurfaceSet => "COMPLEX_TRIANGULATED_SURFACE_SET",
        }
    }

    fn base_name(self) -> &'static str {
        match self {
            Self::Face | Self::ComplexFace => "TESSELLATED_FACE",
            Self::SurfaceSet | Self::ComplexSurfaceSet => "TESSELLATED_SURFACE_SET",
        }
    }

    fn own_parameter_offset(self) -> usize {
        match self {
            Self::Face | Self::ComplexFace => 5,
            Self::SurfaceSet | Self::ComplexSurfaceSet => 4,
        }
    }
}

fn inherited_parameter<'a>(record: &'a RawRecord, entity: &str, index: usize) -> Option<&'a Value> {
    if record.partials.len() == 1 {
        record.parameter(index + 1)
    } else {
        entity_parameter(record, entity, index, 0)
    }
}

fn bytes_for<T>(
    count: usize,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<u64, CodecError> {
    u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn decimal_digits(id: u64) -> u64 {
    if id == 0 {
        1
    } else {
        u64::from(id.ilog10()) + 1
    }
}

fn admitted_mesh_id(
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::tessellation::TessellationId, CodecError> {
    let digits = decimal_digits(id);
    let _key_bytes = ctx.reserve_scoped(digits, "step_tessellation_mesh_key")?;
    ctx.charge_retained(
        u64_from_index("step:tessellation:mesh#".len())
            .checked_add(digits)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("step_tessellation_mesh_id", u64::MAX - 1, u64::MAX)
            })?,
        "step_tessellation_mesh_id",
    )?;
    Ok(ids::tessellation(kind!("mesh"), id).into())
}

fn admitted_surface_id<'a>(
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(cadmpeg_ir::ids::Identity, ScopedReservation<'a>), CodecError> {
    let digits = decimal_digits(id);
    let bytes = u64_from_index("step:data:surface#".len())
        .checked_add(digits)
        .and_then(|length| length.checked_add(digits))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_surface_id", u64::MAX - 1, u64::MAX)
        })?;
    let reservation = ctx.reserve_scoped(bytes, "step_tessellation_surface_id")?;
    Ok((ids::data(kind!("surface"), id), reservation))
}

fn admitted_source_association(
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::SourceObjectAssociation, CodecError> {
    ctx.charge_retained(decimal_digits(id) + 1, "step_tessellation_source_object_id")?;
    Ok(super::step_source_association(id, None))
}

fn push_loss(
    losses: &mut Vec<LossNote>,
    code: StepLossCode,
    message: std::fmt::Arguments<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(losses, 1, "step_tessellation_loss_notes")?;
    let vocabulary_bytes = u64_from_index(code.code().len())
        .checked_add(u64_from_index("step".len()))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_loss_notes", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_retained(vocabulary_bytes, "step_tessellation_loss_notes")?;
    let text = ctx.format_retained(message, "step_tessellation_loss_notes")?;
    losses.push(code.note(text));
    Ok(())
}

fn collect_checked<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    items: impl IntoIterator<Item = T>,
) -> Result<Vec<T>, CodecError> {
    let mut values = ctx.vector_storage(count, operation)?;
    for item in items {
        if values.len() == count {
            return Err(ctx.refuse_codec_limit(operation, 0, 1));
        }
        values.push(item);
    }
    Ok(values)
}

fn collect_optional_checked<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    items: impl IntoIterator<Item = Option<T>>,
) -> Result<Option<Vec<T>>, CodecError> {
    let mut values = ctx.vector_storage(count, operation)?;
    for item in items {
        let Some(item) = item else {
            return Ok(None);
        };
        if values.len() == count {
            return Err(ctx.refuse_codec_limit(operation, 0, 1));
        }
        values.push(item);
    }
    Ok(Some(values))
}

fn collect_result_checked<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
    items: impl IntoIterator<Item = Result<T, CodecError>>,
) -> Result<Vec<T>, CodecError> {
    let mut values = ctx.vector_storage(count, operation)?;
    for item in items {
        let item = item?;
        if values.len() == count {
            return Err(ctx.refuse_codec_limit(operation, 0, 1));
        }
        values.push(item);
    }
    Ok(values)
}

fn coordinate_rows<'a>(
    record: &RawRecord,
    scale: f64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<FinitePoint3>, ScopedReservation<'a>)>, CodecError> {
    for rows in record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
    {
        let mut bytes = {
            ctx.charge_collection_items(
                u64_from_index(rows.len()),
                "step_tessellation_coordinate_rows",
            )?;
            ctx.reserve_scoped(0, "step_tessellation_coordinate_rows")
        }?;
        let vertices = bytes
            .with_storage(|| {
                collect_optional_checked(
                    ctx,
                    rows.len(),
                    "step_tessellation_coordinate_rows",
                    rows.iter().map(|row| {
                        let values = row.list()?;
                        if values.len() != 3 {
                            return None;
                        }
                        let point = Point3::new(
                            values[0].number()? * scale,
                            values[1].number()? * scale,
                            values[2].number()? * scale,
                        );
                        FinitePoint3::new(point)
                    }),
                )
            })?
            .filter(|vertices| !vertices.is_empty());
        if let Some(vertices) = vertices {
            return Ok(Some((vertices, bytes)));
        }
    }
    Ok(None)
}

fn triangle_rows<'a>(
    value: &Value,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let Some(rows) = value.list() else {
        return Ok(None);
    };
    let mut bytes = {
        ctx.charge_collection_items(
            u64_from_index(rows.len()),
            "step_tessellation_triangle_rows",
        )?;
        ctx.reserve_scoped(0, "step_tessellation_triangle_rows")
    }?;
    Ok(bytes
        .with_storage(|| {
            collect_optional_checked(
                ctx,
                rows.len(),
                "step_tessellation_triangle_rows",
                rows.iter().map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    Some([
                        u32::try_from(values[0].integer()?).ok()?,
                        u32::try_from(values[1].integer()?).ok()?,
                        u32::try_from(values[2].integer()?).ok()?,
                    ])
                }),
            )
        })?
        .map(|triangles| AdmittedTriangles { triangles, bytes }))
}

fn complex_triangles<'a>(
    strips: Option<&Value>,
    fans: Option<&Value>,
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let (strips, _strip_rows_bytes, _strip_indices_bytes) =
        index_rows(strips, kind, id, "strip", ctx)?;
    let (fans, _fan_rows_bytes, _fan_indices_bytes) = index_rows(fans, kind, id, "fan", ctx)?;
    let triangle_count = strips
        .iter()
        .chain(fans.iter())
        .try_fold(0usize, |count, row| count.checked_add(row.len() - 2))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_complex_triangle_count", u64::MAX - 1, u64::MAX)
        })?;
    let mut bytes = {
        ctx.charge_collection_items(
            u64_from_index(triangle_count),
            "step_complex_tessellation_triangles",
        )?;
        ctx.reserve_scoped(0, "step_complex_tessellation_triangles")
    }?;
    let mut triangles = Vec::new();

    bytes.with_storage(|| {
        ctx.reserve_capacity(
            &mut triangles,
            triangle_count,
            "step_complex_tessellation_triangles",
        )
    })?;
    for strip in strips {
        for index in 0..strip.len() - 2 {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
    }
    for fan in fans {
        for index in 1..fan.len() - 1 {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
    }
    Ok((!triangles.is_empty()).then_some(AdmittedTriangles { triangles, bytes }))
}

fn index_rows<'a>(
    value: Option<&Value>,
    kind: &str,
    id: u64,
    lane: &str,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<Vec<u32>>, ScopedReservation<'a>, ScopedReservation<'a>), CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok((
            Vec::new(),
            ctx.reserve_scoped(0, "step_complex_tessellation_rows")?,
            ctx.reserve_scoped(0, "step_complex_tessellation_indices")?,
        ));
    };
    let mut row_bytes = {
        ctx.charge_collection_items(u64_from_index(rows.len()), "step_complex_tessellation_rows")?;
        ctx.reserve_scoped(0, "step_complex_tessellation_rows")
    }?;
    let mut index_bytes = ctx.reserve_scoped(0, "step_complex_tessellation_indices")?;
    let indices = row_bytes.with_storage(|| {
        collect_result_checked(
            ctx,
            rows.len(),
            "step_complex_tessellation_rows",
            rows.iter().enumerate().map(|(row_index, row)| {
                let invalid = || {
                    CodecError::malformed(format_args!(
                        "{kind} #{id} {lane} row {} is invalid",
                        row_index + 1
                    ))
                };
                let Some(values) = row.list().filter(|values| values.len() >= 3) else {
                    return Err(invalid());
                };
                ctx.charge_collection_items(
                    u64_from_index(values.len()),
                    "step_complex_tessellation_indices",
                )?;
                index_bytes.with_storage(|| {
                    collect_result_checked(
                        ctx,
                        values.len(),
                        "step_complex_tessellation_indices",
                        values.iter().map(|value| {
                            u32::try_from(value.integer().ok_or_else(invalid)?)
                                .map_err(|_| invalid())
                        }),
                    )
                })
            }),
        )
    })?;
    Ok((indices, row_bytes, index_bytes))
}

fn replicated_normals<'a>(
    count: usize,
    normal: FiniteVector3,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<FiniteVector3>, ScopedReservation<'a>), CodecError> {
    let storage = ctx.reserve_scoped(
        bytes_for::<FiniteVector3>(count, ctx, "step_tessellation_normal_replication")?,
        "step_tessellation_normal_replication",
    )?;
    let normals = ctx.alloc_filled(count, normal, "step_tessellation_normal_replication")?;
    Ok((normals, storage))
}

fn normal_rows<'a>(
    value: Option<&Value>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<FiniteVector3>, ScopedReservation<'a>)>, CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok(None);
    };
    let mut bytes = {
        ctx.charge_collection_items(u64_from_index(rows.len()), "step_tessellation_normal_rows")?;
        ctx.reserve_scoped(0, "step_tessellation_normal_rows")
    }?;
    Ok(bytes
        .with_storage(|| {
            collect_optional_checked(
                ctx,
                rows.len(),
                "step_tessellation_normal_rows",
                rows.iter().map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    let normal = Vector3::new(
                        values[0].number()?,
                        values[1].number()?,
                        values[2].number()?,
                    );
                    super::geometry::normalize(normal).map(FiniteVector3::from)
                }),
            )
        })?
        .map(|normals| (normals, bytes)))
}
#[cfg(test)]
pub(crate) mod tests;
