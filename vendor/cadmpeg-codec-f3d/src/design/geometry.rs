// SPDX-License-Identifier: Apache-2.0
//! Sketch-arrangement and profile-containment computational geometry.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_i64, truncate_f64_to_usize};
use cadmpeg_core::decode::index_from_u32;

use crate::design::profile_select::historical_face_points;
use crate::records::{
    sketch_relations::SketchRelationOperand,
    topology::extrude_selection::DesignExtrudeSelectionMember,
};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::PositiveLength;
use std::collections::{HashMap, HashSet};

const EPS_GEOMETRY_CERTIFIED_ANALYTIC_LOOP_E6: f64 = 1.0e-6;
const EPS_GEOMETRY_CERTIFIED_CIRCLE_E6: f64 = 1.0e-6;
const EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12: f64 = 1.0e-12;
const EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10: f64 = 1.0e-10;

macro_rules! geometric {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Format-side work cap for arrangement edge retention walks.
///
/// The local walk slice and the caller's session work allowance are charged separately.
// SolidCraft: raised from 1_000_000 so sketch-heavy Fusion files decode (see vendor/README.md).
pub(crate) const MAX_ARRANGEMENT_WALK_WORK: usize = 128_000_000;

#[derive(Clone)]
struct SketchArrangementEdge {
    nodes: [usize; 2],
    boundary: cadmpeg_ir::features::SketchProfileBoundaryUse,
    polyline: Vec<Point2>,
}

struct SketchArrangementFace {
    boundary: Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    polyline: Vec<Point2>,
}

fn copy_arrangement_boundary(
    ctx: &DecodeContext<'_>,
    boundary: &cadmpeg_ir::features::SketchProfileBoundaryUse,
) -> Result<cadmpeg_ir::features::SketchProfileBoundaryUse, CodecError> {
    Ok(cadmpeg_ir::features::SketchProfileBoundaryUse {
        entity: (boundary.entity)
            .try_clone_for_decode(ctx, "f3d arrangement boundary entity id")?,
        parameter_range: boundary.parameter_range,
        reversed: boundary.reversed,
    })
}

fn copy_arrangement_boundary_run(
    ctx: &DecodeContext<'_>,
    boundaries: &[cadmpeg_ir::features::SketchProfileBoundaryUse],
) -> Result<Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>, CodecError> {
    let mut copy = Vec::new();
    for boundary in boundaries {
        let boundary = copy_arrangement_boundary(ctx, boundary)?;
        ctx.push_vec(&mut copy, boundary, "f3d arrangement selected boundary")?;
    }
    Ok(copy)
}

pub(super) fn arrangement_region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point2],
    tolerance: f64,
    budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::SketchProfileRegion>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let Some(faces) = sketch_arrangement_faces(sketch, entities, tolerance, budget, ctx)? else {
        return Ok(None);
    };
    let mut boundary = None;
    let mut boundary_count = 0;
    for face in &faces {
        let mut matches = true;
        for point in points {
            let mut on_boundary = false;
            for use_ in &face.boundary {
                if point_on_profile_boundary_use(ctx, *point, use_, entities, tolerance)? {
                    on_boundary = true;
                    break;
                }
            }
            if !on_boundary {
                matches = false;
                break;
            }
        }
        if matches {
            boundary_count += 1;
            boundary = Some(face);
            if boundary_count == 2 {
                break;
            }
        }
    }
    if boundary_count == 1 {
        let face = geometric!(boundary);
        let boundary = copy_arrangement_boundary_run(ctx, &face.boundary)?;
        return Ok(SketchProfileRegion::trimmed(boundary, Vec::new()).ok());
    }
    let mut interior = None;
    let mut interior_count = 0;
    for face in &faces {
        let mut matches = true;
        for point in points {
            let mut on_boundary = false;
            for use_ in &face.boundary {
                if point_on_profile_boundary_use(ctx, *point, use_, entities, tolerance)? {
                    on_boundary = true;
                    break;
                }
            }
            if on_boundary || !point_in_polygon(*point, &face.polyline) {
                matches = false;
                break;
            }
        }
        if matches {
            interior_count += 1;
            interior = Some(face);
            if interior_count == 2 {
                break;
            }
        }
    }
    if interior_count != 1 {
        return Ok(None);
    }
    let face = geometric!(interior);
    let boundary = copy_arrangement_boundary_run(ctx, &face.boundary)?;
    Ok(SketchProfileRegion::trimmed(boundary, Vec::new()).ok())
}

fn sketch_arrangement_faces(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<SketchArrangementFace>>, CodecError> {
    use cadmpeg_ir::features::SketchProfileBoundaryUse;
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometryDefinition};

    let mut nodes = Vec::<Point2>::new();
    let mut pending = Vec::<SketchProfileBoundaryUse>::new();
    let mut circles = Vec::new();
    let mut candidate_uses = Vec::new();
    if sketch.profiles.is_empty() {
        for entity in entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id && !entity.construction)
            .filter(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                ) || sketch_geometry_parameter_range(&entity.geometry).is_some()
            })
        {
            let entity =
                (entity.id()).try_clone_for_decode(ctx, "f3d arrangement candidate entity id")?;
            ctx.push_vec(
                &mut candidate_uses,
                SketchEntityUse {
                    entity,
                    reversed: false,
                },
                "f3d arrangement candidate use",
            )?;
        }
    } else {
        for use_ in sketch.profiles.iter().flat_map(|profile| profile.iter()) {
            let entity =
                (use_.entity).try_clone_for_decode(ctx, "f3d arrangement candidate entity id")?;
            ctx.push_vec(
                &mut candidate_uses,
                SketchEntityUse {
                    entity,
                    reversed: use_.reversed,
                },
                "f3d arrangement candidate use",
            )?;
        }
    }
    for use_ in candidate_uses {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        if let SketchGeometryDefinition::Circle { center, radius } = *entity.geometry.definition() {
            ctx.push_vec(
                &mut circles,
                (use_, center.get(), radius),
                "f3d arrangement circle",
            )?;
            continue;
        }
        let range = geometric!(sketch_geometry_parameter_range(&entity.geometry));
        for point in [
            geometric!(sketch_geometry_point(&entity.geometry, range[0], ctx)?),
            geometric!(sketch_geometry_point(&entity.geometry, range[1], ctx)?),
        ] {
            arrangement_node(&mut nodes, point, tolerance, ctx)?;
        }
        let id = (entity.id()).try_clone_for_decode(ctx, "f3d arrangement pending entity id")?;
        ctx.push_vec(
            &mut pending,
            SketchProfileBoundaryUse {
                entity: id,
                parameter_range: geometric!(cadmpeg_ir::geometry::DirectedParameterRange::new(
                    range
                )
                .ok()),
                reversed: use_.reversed,
            },
            "f3d arrangement pending boundary",
        )?;
    }
    for (use_, center, radius) in circles {
        if radius.get() <= tolerance {
            return Ok(None);
        }
        let mut angles = arrangement_circle_angles(&nodes, center, radius, tolerance, ctx)?;
        if angles.len() < 2 {
            let additional = if angles.is_empty() {
                [Some(0.0), Some(std::f64::consts::PI)]
            } else {
                [Some(angles[0] + std::f64::consts::PI), None]
            };
            for angle in additional.into_iter().flatten() {
                arrangement_node(
                    &mut nodes,
                    Point2::new(
                        center.u + radius.get() * angle.cos(),
                        center.v + radius.get() * angle.sin(),
                    ),
                    tolerance,
                    ctx,
                )?;
            }
            angles = arrangement_circle_angles(&nodes, center, radius, tolerance, ctx)?;
        }
        if angles.len() < 2 {
            return Ok(None);
        }
        for index in 0..angles.len() {
            let start = angles[index];
            let mut end = angles[(index + 1) % angles.len()];
            if index + 1 == angles.len() {
                end += std::f64::consts::TAU;
            }
            let range = [start, end];
            let id =
                (use_.entity).try_clone_for_decode(ctx, "f3d arrangement pending entity id")?;
            ctx.push_vec(
                &mut pending,
                SketchProfileBoundaryUse {
                    entity: id,
                    parameter_range: geometric!(cadmpeg_ir::geometry::DirectedParameterRange::new(
                        range
                    )
                    .ok()),
                    reversed: use_.reversed,
                },
                "f3d arrangement pending boundary",
            )?;
        }
    }
    let mut split_pending = Vec::new();
    for boundary in pending {
        let entity = geometric!(entities
            .iter()
            .find(|entity| entity.id() == &boundary.entity));
        let parameters = geometric!(arrangement_split_parameters(
            &entity.geometry,
            boundary.parameter_range.endpoints(),
            &nodes,
            tolerance,
            ctx,
        )?);
        for parameters in parameters.windows(2) {
            let range = [parameters[0], parameters[1]];
            let id =
                (boundary.entity).try_clone_for_decode(ctx, "f3d arrangement split entity id")?;
            let parameter_range =
                geometric!(cadmpeg_ir::geometry::DirectedParameterRange::new(range).ok());
            let polyline = geometric!(profile_use_polyline(
                entity,
                range,
                boundary.reversed,
                tolerance,
                ctx,
            )?);
            ctx.push_vec(
                &mut split_pending,
                (
                    SketchProfileBoundaryUse {
                        entity: id,
                        parameter_range,
                        reversed: boundary.reversed,
                    },
                    polyline,
                ),
                "f3d arrangement split boundary",
            )?;
        }
    }
    let mut edges = Vec::<SketchArrangementEdge>::new();
    for (boundary, polyline) in split_pending {
        let edge = SketchArrangementEdge {
            nodes: [
                arrangement_node(&mut nodes, *geometric!(polyline.first()), tolerance, ctx)?,
                arrangement_node(&mut nodes, *geometric!(polyline.last()), tolerance, ctx)?,
            ],
            boundary,
            polyline,
        };
        if edge.nodes[0] == edge.nodes[1] {
            return Ok(None);
        }
        let mut coincident = false;
        for candidate in &edges {
            if (candidate.nodes == edge.nodes || candidate.nodes == [edge.nodes[1], edge.nodes[0]])
                && arrangement_edges_coincident(candidate, &edge, entities, tolerance, ctx)?
            {
                coincident = true;
                break;
            }
        }
        if coincident {
            continue;
        }
        ctx.push_vec(&mut edges, edge, "f3d arrangement edge")?;
    }
    arrangement_retain_cycle_edges(&mut edges, nodes.len(), budget, ctx)?;
    if edges.len() < 3 {
        return Ok(None);
    }
    let mut edge_tubes = Vec::new();
    for edge in &edges {
        let Some(tubes) = arrangement_edge_tubes(edge, entities, tolerance, ctx)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut edge_tubes, tubes, "f3d arrangement edge tube set")?;
    }
    let mut edge_bounds = Vec::new();
    for tubes in &edge_tubes {
        let bounds = geometric!(certified_tube_bounds(tubes));
        ctx.push_vec(&mut edge_bounds, bounds, "f3d arrangement edge bounds")?;
    }
    for left_index in 0..edges.len() {
        for right_index in left_index + 1..edges.len() {
            let left = &edges[left_index];
            let right = &edges[right_index];
            let mut shared_nodes = [0; 2];
            let mut shared_count = 0;
            for node in left.nodes {
                if right.nodes.contains(&node) {
                    shared_nodes[shared_count] = node;
                    shared_count += 1;
                }
            }
            if shared_count != 0 {
                if !arrangement_edges_meet_only_at_nodes(
                    left,
                    right,
                    entities,
                    &nodes,
                    &shared_nodes[..shared_count],
                    tolerance,
                ) {
                    return Ok(None);
                }
                continue;
            }
            let left_bounds = edge_bounds[left_index];
            let right_bounds = edge_bounds[right_index];
            if left_bounds[1].u < right_bounds[0].u
                || right_bounds[1].u < left_bounds[0].u
                || left_bounds[1].v < right_bounds[0].v
                || right_bounds[1].v < left_bounds[0].v
            {
                continue;
            }
            if !arrangement_edges_proven_disjoint(
                left,
                right,
                entities,
                &edge_tubes[left_index],
                &edge_tubes[right_index],
            ) {
                return Ok(None);
            }
        }
    }
    let mut outgoing = ctx.alloc_filled(
        nodes.len(),
        Vec::<(usize, bool, f64)>::new(),
        "f3d_arrangement_outgoing",
    )?;
    let _outgoing_reservation = Some(
        ({
            let bytes = u64::try_from(edges.len())
                .ok()
                .and_then(|count| count.checked_mul(2))
                .and_then(|count| {
                    count
                        .checked_mul(u64::try_from(std::mem::size_of::<(usize, bool, f64)>()).ok()?)
                })
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("f3d arrangement outgoing bytes", u64::MAX - 1, u64::MAX)
                })?;
            ctx.reserve_scoped(bytes, "f3d arrangement outgoing entries")
        })?,
    );
    for (edge_index, edge) in edges.iter().enumerate() {
        let forward = geometric!(edge.polyline.get(1));
        let reverse = geometric!(edge
            .polyline
            .get(geometric!(edge.polyline.len().checked_sub(2))));
        ctx.push_vec(
            &mut outgoing[edge.nodes[0]],
            (
                edge_index,
                false,
                (forward.v - nodes[edge.nodes[0]].v).atan2(forward.u - nodes[edge.nodes[0]].u),
            ),
            "f3d arrangement outgoing entry",
        )?;
        ctx.push_vec(
            &mut outgoing[edge.nodes[1]],
            (
                edge_index,
                true,
                (reverse.v - nodes[edge.nodes[1]].v).atan2(reverse.u - nodes[edge.nodes[1]].u),
            ),
            "f3d arrangement outgoing entry",
        )?;
    }
    if edges
        .iter()
        .any(|edge| edge.nodes.iter().any(|node| outgoing[*node].len() < 2))
    {
        return Ok(None);
    }
    for uses in &mut outgoing {
        ctx.stable_sort_by(
            &mut uses[..],
            |left, right| left.2.total_cmp(&right.2),
            |_| 0,
            "sort f3d design geometry 1",
        )?;
    }
    let mut visited = ctx.alloc_filled(edges.len(), [false; 2], "f3d arrangement edge visits")?;
    let mut faces = Vec::new();
    for edge_index in 0..edges.len() {
        for reversed in [false, true] {
            if visited[edge_index][usize::from(reversed)] {
                continue;
            }
            let start = (edge_index, reversed);
            let mut current = start;
            let mut boundary = Vec::new();
            let mut polyline = Vec::new();
            loop {
                let (index, reverse) = current;
                if visited[index][usize::from(reverse)] {
                    if current != start {
                        return Ok(None);
                    }
                    break;
                }
                visited[index][usize::from(reverse)] = true;
                let edge = &edges[index];
                let mut use_ = copy_arrangement_boundary(ctx, &edge.boundary)?;
                if reverse {
                    use_.reversed = !use_.reversed;
                }
                ctx.push_vec(&mut boundary, use_, "f3d arrangement face boundary")?;
                if reverse {
                    for point in edge.polyline.iter().rev().take(edge.polyline.len() - 1) {
                        ctx.push_vec(&mut polyline, *point, "f3d arrangement face point")?;
                    }
                } else {
                    for point in edge.polyline.iter().take(edge.polyline.len() - 1) {
                        ctx.push_vec(&mut polyline, *point, "f3d arrangement face point")?;
                    }
                }
                let destination = edge.nodes[usize::from(!reverse)];
                let uses = &outgoing[destination];
                let twin = geometric!(uses.iter().position(|(candidate, candidate_reverse, _)| {
                    *candidate == index && *candidate_reverse != reverse
                }));
                let next = uses[(twin + uses.len() - 1) % uses.len()];
                current = (next.0, next.1);
            }
            if polyline.len() >= 3 && signed_polygon_area(&polyline) > tolerance * tolerance {
                ctx.push_vec(
                    &mut faces,
                    SketchArrangementFace { boundary, polyline },
                    "f3d arrangement face",
                )?;
            }
        }
    }
    Ok((!faces.is_empty()).then_some(faces))
}

fn arrangement_edges_meet_only_at_nodes(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    if arrangement_line_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_line_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    if arrangement_arc_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_arc_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    let Some(left_segment) = arrangement_analytic_segment(left, entities) else {
        return false;
    };
    let Some(right_segment) = arrangement_analytic_segment(right, entities) else {
        return false;
    };
    if let (
        ProfileBoundarySegment::Arc {
            center: left_center,
            radius: left_radius,
            start_angle: left_start,
            end_angle: left_end,
            ..
        },
        ProfileBoundarySegment::Arc {
            center: right_center,
            radius: right_radius,
            start_angle: right_start,
            end_angle: right_end,
            ..
        },
    ) = (&left_segment, &right_segment)
    {
        if left_center == right_center && left_radius == right_radius {
            let strictly_inside = |angle: f64, start: f64, end: f64| {
                angle_strictly_inside_arc(angle, start, end, left_radius.get(), tolerance)
            };
            return !strictly_inside(*left_start, *right_start, *right_end)
                && !strictly_inside(*left_end, *right_start, *right_end)
                && !strictly_inside(*right_start, *left_start, *left_end)
                && !strictly_inside(*right_end, *left_start, *left_end);
        }
    }
    analytic_segment_intersections(&left_segment, &right_segment).is_some_and(|intersections| {
        intersections.iter().flatten().all(|intersection| {
            shared_nodes
                .iter()
                .any(|node| point_distance(*intersection, nodes[*node]) <= tolerance)
        })
    })
}

/// Whether `angle` lies strictly inside the directed arc from `start` to `end`
/// on a circle of radius `radius`, with `tolerance` a length on that circle.
///
/// The arc's length is positive and finite: a sketch circle or arc has a
/// positive finite radius and a parameter range with distinct finite
/// endpoints. An arc no longer than twice the tolerance has no strict
/// interior, because every point of it is then within `tolerance` of an
/// endpoint; that is the state the arc-length fraction has no answer for, and
/// it is answered here rather than capped.
fn angle_strictly_inside_arc(
    angle: f64,
    start: f64,
    end: f64,
    radius: f64,
    tolerance: f64,
) -> bool {
    let arc_length = radius * (end - start).abs();
    if arc_length <= 2.0 * tolerance {
        return false;
    }
    let parameter_tolerance = tolerance / arc_length;
    directed_angle_parameter(angle, start, end).is_some_and(|parameter| {
        parameter > parameter_tolerance && parameter < 1.0 - parameter_tolerance
    })
}

fn arrangement_arc_nurbs_meet_only_at_endpoint(
    arc: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(arc_entity) = entities
        .iter()
        .find(|entity| entity.id() == &arc.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id() == &nurbs.boundary.entity)
    else {
        return false;
    };
    let (center, radius) = match *arc_entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => (center.get(), radius.get()),
        _ => return false,
    };
    let SketchGeometryDefinition::Nurbs { curve } = nurbs_entity.geometry.definition() else {
        return false;
    };
    if curve.periodic() {
        return false;
    }
    if curve
        .weights()
        .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
        || sketch_geometry_parameter_range(&nurbs_entity.geometry)
            != Some(nurbs.boundary.parameter_range.endpoints())
    {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    if (point_distance(center, shared) - radius).abs() > tolerance {
        return false;
    }
    let first_shared = point_distance(curve.control_points()[0].get(), shared) <= tolerance;
    let last_shared = curve
        .control_points()
        .last()
        .is_some_and(|point| point_distance(point.get(), shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let endpoint = if first_shared {
        0
    } else {
        curve.pole_rows().count() - 1
    };
    let normal = Point2::new(shared.u - center.u, shared.v - center.v);
    let support = |point: Point2| normal.u * (point.u - shared.u) + normal.v * (point.v - shared.v);
    let threshold = tolerance * radius;
    support(curve.control_points()[endpoint].get()).abs() <= threshold
        && (curve
            .control_points()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != endpoint)
            .all(|(_, point)| support(point.get()) > threshold)
            || curve
                .control_points()
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != endpoint)
                .all(|(_, point)| point_distance(center, point.get()) < radius - tolerance))
}

fn arrangement_line_nurbs_meet_only_at_endpoint(
    line: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(line_entity) = entities
        .iter()
        .find(|entity| entity.id() == &line.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id() == &nurbs.boundary.entity)
    else {
        return false;
    };
    let SketchGeometryDefinition::Line { start, end } = *line_entity.geometry.definition() else {
        return false;
    };
    let (start, end) = (start.get(), end.get());
    let SketchGeometryDefinition::Nurbs { curve } = nurbs_entity.geometry.definition() else {
        return false;
    };
    if curve.periodic() {
        return false;
    }
    if curve
        .weights()
        .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
    {
        return false;
    }
    let Some(domain) = sketch_geometry_parameter_range(&nurbs_entity.geometry) else {
        return false;
    };
    if nurbs.boundary.parameter_range.endpoints() != domain {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    let control_points = curve.pole_rows().raw_points();
    let first_shared = point_distance(control_points[0], shared) <= tolerance;
    let last_shared = control_points
        .last()
        .is_some_and(|point| point_distance(*point, shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let line_length = point_distance(start, end);
    if line_length <= tolerance {
        return false;
    }
    let side =
        |point: Point2| direction.u * (point.v - start.v) - direction.v * (point.u - start.u);
    let endpoint = if first_shared {
        0
    } else {
        control_points.len() - 1
    };
    let threshold = tolerance * line_length;
    if side(control_points[endpoint]).abs() > threshold {
        return false;
    }
    let mut signs = control_points
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != endpoint)
        .map(|(_, point)| side(*point));
    let Some(first_side) = signs.next() else {
        return false;
    };
    first_side.abs() > threshold
        && signs.all(|candidate| {
            candidate.abs() > threshold
                && candidate.is_sign_positive() == first_side.is_sign_positive()
        })
}

fn analytic_segment_intersections(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    match (left, right) {
        (
            ProfileBoundarySegment::Line { start: a, end: b },
            ProfileBoundarySegment::Line { start: c, end: d },
        ) => {
            let [parameter, other_parameter] =
                cadmpeg_ir::math::planar::line_line_parameters(*a, *b, *c, *d)?
                    .map(cadmpeg_ir::scalar::FiniteReal::get);
            if !(0.0..=1.0).contains(&parameter) || !(0.0..=1.0).contains(&other_parameter) {
                return Some([None; 2]);
            }
            Some([
                Some(Point2::new(
                    cadmpeg_ir::math::interpolate(a.u, b.u, parameter)?.get(),
                    cadmpeg_ir::math::interpolate(a.v, b.v, parameter)?.get(),
                )),
                None,
            ])
        }
        (ProfileBoundarySegment::Line { start, end }, arc @ ProfileBoundarySegment::Arc { .. })
        | (arc @ ProfileBoundarySegment::Arc { .. }, ProfileBoundarySegment::Line { start, end }) => {
            line_arc_intersection_points((*start, *end), arc)
        }
        (left @ ProfileBoundarySegment::Arc { .. }, right @ ProfileBoundarySegment::Arc { .. }) => {
            arc_intersection_points(left, right)
        }
    }
}

fn line_arc_intersection_points(
    (start, end): (Point2, Point2),
    arc: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = arc
    else {
        return None;
    };
    let radius = radius.get();
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    if start == end {
        return Some([
            (offset.u.hypot(offset.v) == radius
                && directed_angle_parameter(offset.v.atan2(offset.u), *start_angle, *end_angle)
                    .is_some())
            .then_some(start),
            None,
        ]);
    }
    let mut points = [None; 2];
    let mut point_count = 0;
    let Some(parameters) =
        cadmpeg_ir::math::planar::line_circle_intersections(start, end, *center, radius)
    else {
        return Some(points);
    };
    // A segment end that is not finite has no distance to measure.
    let segment =
        cadmpeg_ir::units::FinitePoint2::new(start).zip(cadmpeg_ir::units::FinitePoint2::new(end));
    for (_, finite_point) in parameters {
        let point = finite_point.get();
        if segment.is_some_and(|(start, end)| {
            cadmpeg_ir::math::planar::point_segment_distance(finite_point, start, end)
                <= 128.0 * f64::EPSILON * radius
        }) {
            let radial = (point.u - center.u).hypot(point.v - center.v);
            if (radial - radius).abs() <= 128.0 * f64::EPSILON * radius
                && directed_angle_parameter(
                    (point.v - center.v).atan2(point.u - center.u),
                    *start_angle,
                    *end_angle,
                )
                .is_some()
                && !points.contains(&Some(point))
            {
                points[point_count] = Some(point);
                point_count += 1;
            }
        }
    }
    Some(points)
}

fn arc_intersection_points(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    let ProfileBoundarySegment::Arc {
        center: lc,
        radius: lr,
        start_angle: ls,
        end_angle: le,
    } = left
    else {
        return None;
    };
    let ProfileBoundarySegment::Arc {
        center: rc,
        radius: rr,
        start_angle: rs,
        end_angle: re,
    } = right
    else {
        return None;
    };
    Some(
        cadmpeg_ir::math::planar::circle_intersections(*lc, lr.get(), *rc, rr.get())?.map(
            |point| {
                point
                    .map(cadmpeg_ir::units::FinitePoint2::get)
                    .filter(|point| {
                        directed_angle_parameter((point.v - lc.v).atan2(point.u - lc.u), *ls, *le)
                            .is_some()
                            && directed_angle_parameter(
                                (point.v - rc.v).atan2(point.u - rc.u),
                                *rs,
                                *re,
                            )
                            .is_some()
                    })
            },
        ),
    )
}

fn arrangement_split_parameters(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
    nodes: &[Point2],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<f64>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut parameters = Vec::new();
    {
        ctx.reserve_vec(&mut parameters, 2, "f3d arrangement split endpoints")?;
    }
    parameters.extend(range);
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            if point_distance(start.get(), end.get()) <= tolerance {
                return Ok(None);
            }
            for point in nodes {
                let parameter = geometric!(cadmpeg_ir::math::planar::line_projection_parameter(
                    start.get(),
                    end.get(),
                    *point,
                ))
                .get();
                if parameter > 0.0
                    && parameter < 1.0
                    && point_segment_distance(*point, (start.get(), end.get())) <= tolerance
                {
                    {
                        ctx.reserve_vec(&mut parameters, 1, "f3d arrangement split parameter")?;
                    }
                    parameters.push(parameter);
                }
            }
        }
        SketchGeometryDefinition::Arc { center, radius, .. } => {
            for point in nodes {
                if (point_distance(*point, center.get()) - radius.get()).abs() > tolerance {
                    continue;
                }
                let angle = (point.v - center.v).atan2(point.u - center.u);
                if let Some(parameter) = directed_angle_parameter(angle, range[0], range[1]) {
                    if parameter > 0.0 && parameter < 1.0 {
                        {
                            ctx.reserve_vec(&mut parameters, 1, "f3d arrangement split parameter")?;
                        }
                        parameters.push(range[0] + parameter * (range[1] - range[0]));
                    }
                }
            }
        }
        _ => {}
    }
    ctx.stable_sort_by(
        &mut parameters[..],
        |left, right| {
            ((left - range[0]) / (range[1] - range[0]))
                .total_cmp(&((right - range[0]) / (range[1] - range[0])))
        },
        |_| 0,
        "sort f3d design geometry 2",
    )?;
    let parameter_tolerance =
        tolerance / geometric!(sketch_geometry_speed_bound(ctx, geometry, range)?).max(tolerance);
    parameters.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    Ok((parameters.len() >= 2).then_some(parameters))
}

fn arrangement_circle_angles(
    nodes: &[Point2],
    center: Point2,
    radius: PositiveLength,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<f64>, CodecError> {
    let mut angles = Vec::new();
    for point in nodes
        .iter()
        .filter(|point| (point_distance(**point, center) - radius.get()).abs() <= tolerance)
    {
        let angle = (point.v - center.v)
            .atan2(point.u - center.u)
            .rem_euclid(std::f64::consts::TAU);
        ctx.push_vec(&mut angles, angle, "f3d arrangement circle angle")?;
    }
    ctx.stable_sort_by(
        &mut angles[..],
        f64::total_cmp,
        |_| 0,
        "sort f3d arrangement circle angles",
    )?;
    angles.dedup_by(|left, right| (*left - *right).abs() <= tolerance / radius.get());
    Ok(angles)
}

fn arrangement_node(
    nodes: &mut Vec<Point2>,
    point: Point2,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<usize, CodecError> {
    if let Some(index) = nodes
        .iter()
        .position(|candidate| point_distance(*candidate, point) <= tolerance)
    {
        return Ok(index);
    }
    ctx.push_vec(nodes, point, "f3d arrangement node")?;
    Ok(nodes.len() - 1)
}

fn arrangement_cycle_work(
    edge_count: usize,
    node_count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<usize, CodecError> {
    edge_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(edge_count))
        .and_then(|count| {
            node_count
                .checked_add(1)
                .and_then(|nodes| count.checked_mul(nodes))
        })
        .ok_or_else(|| ctx.refuse_codec_limit("F3D arrangement cycle work", 0, u64::MAX))
}

/// Remove curve fragments that cannot bound a planar face.
///
/// A sketch can contain construction-like open branches in the solved entity
/// stream even when the source profile table is empty. Such branches are graph
/// bridges, not profile boundaries. Retaining only edges with an alternate
/// path between their endpoints leaves the bounded-face arrangement intact
/// while preserving intersections and nested loops.
fn arrangement_retain_cycle_edges(
    edges: &mut Vec<SketchArrangementEdge>,
    node_count: usize,
    budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    loop {
        // One BFS per edge scans all edges at each visited node: O(E² V).
        // The bound also admits visit initialization and retention scans.
        let work = arrangement_cycle_work(edges.len(), node_count, ctx)?;
        let remaining = budget.remaining();
        if !budget.charge_by(work) {
            return Err(ctx.resource_refusal().map_or_else(
                || {
                    ctx.refuse_codec_limit(
                        "F3D arrangement cycle work",
                        cadmpeg_core::decode::u64_from_index(remaining),
                        cadmpeg_core::decode::u64_from_index(work),
                    )
                },
                CodecError::ResourceLimit,
            ));
        }
        let mut keep = Vec::new();
        for (index, edge) in edges.iter().enumerate() {
            let has_cycle = arrangement_has_alternate_path(
                edges,
                index,
                edge.nodes[0],
                edge.nodes[1],
                node_count,
                ctx,
            )?;
            ctx.push_vec(&mut keep, has_cycle, "f3d arrangement retained edge mark")?;
        }
        if keep.iter().all(|keep| *keep) {
            break;
        }
        let mut retained = Vec::new();
        for (edge, keep) in std::mem::take(edges).into_iter().zip(keep) {
            if keep {
                ctx.push_vec(&mut retained, edge, "f3d arrangement retained edge")?;
            }
        }
        *edges = retained;
    }
    Ok(())
}

fn arrangement_has_alternate_path(
    edges: &[SketchArrangementEdge],
    excluded_edge: usize,
    start: usize,
    destination: usize,
    node_count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut visited = ctx.alloc_filled(node_count, false, "f3d arrangement visit marks")?;
    let mut pending = Vec::new();
    ctx.push_vec(&mut pending, start, "f3d arrangement pending nodes")?;
    visited[start] = true;
    while let Some(node) = pending.pop() {
        if node == destination {
            return Ok(true);
        }
        for (index, edge) in edges.iter().enumerate() {
            if index == excluded_edge {
                continue;
            }
            let Some(next) = (edge.nodes[0] == node)
                .then_some(edge.nodes[1])
                .or_else(|| (edge.nodes[1] == node).then_some(edge.nodes[0]))
            else {
                continue;
            };
            if !visited[next] {
                visited[next] = true;
                ctx.push_vec(&mut pending, next, "f3d arrangement pending nodes")?;
            }
        }
    }
    Ok(false)
}

fn arrangement_edges_coincident(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if left.boundary.entity == right.boundary.entity
        && left.boundary.parameter_range.endpoints() == right.boundary.parameter_range.endpoints()
    {
        return Ok(true);
    }
    let Some(left_entity) = entities
        .iter()
        .find(|entity| entity.id() == &left.boundary.entity)
    else {
        return Ok(false);
    };
    let Some(right_entity) = entities
        .iter()
        .find(|entity| entity.id() == &right.boundary.entity)
    else {
        return Ok(false);
    };
    Ok(
        match (
            left_entity.geometry.definition(),
            right_entity.geometry.definition(),
        ) {
            (SketchGeometryDefinition::Line { .. }, SketchGeometryDefinition::Line { .. }) => {
                let left_midpoint = sketch_geometry_point(
                    &left_entity.geometry,
                    (left.boundary.parameter_range.endpoints()[0]
                        + left.boundary.parameter_range.endpoints()[1])
                        * 0.5,
                    ctx,
                )?;
                left_midpoint
                    .zip(right.polyline.last().copied())
                    .is_some_and(|(point, end)| {
                        point_segment_distance(point, (right.polyline[0], end)) <= tolerance
                    })
            }
            (
                SketchGeometryDefinition::Circle {
                    center: left_center,
                    radius: left_radius,
                }
                | SketchGeometryDefinition::Arc {
                    center: left_center,
                    radius: left_radius,
                    ..
                },
                SketchGeometryDefinition::Circle {
                    center: right_center,
                    radius: right_radius,
                }
                | SketchGeometryDefinition::Arc {
                    center: right_center,
                    radius: right_radius,
                    ..
                },
            ) => {
                point_distance(left_center.get(), right_center.get()) <= tolerance
                    && (left_radius.get() - right_radius.get()).abs() <= tolerance
                    && ((left.boundary.parameter_range.endpoints()[1]
                        - left.boundary.parameter_range.endpoints()[0])
                        .abs()
                        - (right.boundary.parameter_range.endpoints()[1]
                            - right.boundary.parameter_range.endpoints()[0])
                            .abs())
                    .abs()
                        <= tolerance / left_radius.get()
                    && sketch_geometry_point(
                        &left_entity.geometry,
                        (left.boundary.parameter_range.endpoints()[0]
                            + left.boundary.parameter_range.endpoints()[1])
                            * 0.5,
                        ctx,
                    )?
                    .zip(sketch_geometry_point(
                        &right_entity.geometry,
                        (right.boundary.parameter_range.endpoints()[0]
                            + right.boundary.parameter_range.endpoints()[1])
                            * 0.5,
                        ctx,
                    )?)
                    .is_some_and(|(left, right)| point_distance(left, right) <= tolerance)
            }
            _ => false,
        },
    )
}

fn arrangement_edges_proven_disjoint(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    left_tubes: &[CertifiedCurveTube],
    right_tubes: &[CertifiedCurveTube],
) -> bool {
    match (
        arrangement_analytic_segment(left, entities),
        arrangement_analytic_segment(right, entities),
    ) {
        (Some(left), Some(right)) => !boundary_segments_intersect(&left, &right),
        _ => left_tubes.iter().all(|left| {
            right_tubes.iter().all(|right| {
                segment_distance((left.start, left.end), (right.start, right.end))
                    > left.error + right.error
            })
        }),
    }
}

fn certified_tube_bounds(tubes: &[CertifiedCurveTube]) -> Option<[Point2; 2]> {
    let first = tubes.first()?;
    Some(tubes.iter().fold(
        [
            Point2::new(
                first.start.u.min(first.end.u) - first.error,
                first.start.v.min(first.end.v) - first.error,
            ),
            Point2::new(
                first.start.u.max(first.end.u) + first.error,
                first.start.v.max(first.end.v) + first.error,
            ),
        ],
        |mut bounds, tube| {
            bounds[0].u = bounds[0].u.min(tube.start.u.min(tube.end.u) - tube.error);
            bounds[0].v = bounds[0].v.min(tube.start.v.min(tube.end.v) - tube.error);
            bounds[1].u = bounds[1].u.max(tube.start.u.max(tube.end.u) + tube.error);
            bounds[1].v = bounds[1].v.max(tube.start.v.max(tube.end.v) + tube.error);
            bounds
        },
    ))
}

fn arrangement_edge_tubes(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<CertifiedCurveTube>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let entity = geometric!(entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity));
    let scale = edge
        .polyline
        .iter()
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0_f64, f64::max);
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    Ok(match entity.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some(vec![CertifiedCurveTube {
            start: edge.polyline[0],
            end: *geometric!(edge.polyline.last()),
            error: 0.0,
        }]),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => certified_arc_tubes(
            center.get(),
            *radius,
            edge.boundary.parameter_range.endpoints()[0],
            edge.boundary.parameter_range.endpoints()[1],
            target_error,
            ctx,
        )?,
        SketchGeometryDefinition::Nurbs { curve }
            if !curve.periodic()
                && sketch_geometry_parameter_range(&entity.geometry)
                    == Some(edge.boundary.parameter_range.endpoints()) =>
        {
            return certified_nurbs_tubes(curve, target_error, ctx);
        }
        _ => None,
    })
}

fn arrangement_analytic_segment(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<ProfileBoundarySegment> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let entity = entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity)?;
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some(ProfileBoundarySegment::Line {
            start: edge.polyline[0],
            end: *edge.polyline.last()?,
        }),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            Some(ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle: edge.boundary.parameter_range.endpoints()[0],
                end_angle: edge.boundary.parameter_range.endpoints()[1],
            })
        }
        _ => None,
    }
}

fn sketch_geometry_parameter_range(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<[f64; 2]> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some([0.0, 1.0]),
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
        } => Some([start_angle.get(), end_angle.get()]),
        SketchGeometryDefinition::Ellipse {
            bounds: Some([start, end]),
            ..
        } => Some([start.get(), end.get()]),
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => Some([
            curve.knots()[index_from_u32(curve.degree())],
            curve.knots()[curve.pole_rows().count()],
        ]),
        _ => None,
    }
}

fn profile_use_polyline(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    range: [f64; 2],
    reversed: bool,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point2>>, CodecError> {
    let travel = geometric!(sketch_geometry_speed_bound(ctx, &entity.geometry, range)?)
        * (range[1] - range[0]).abs();
    let ordinary_midpoint = (range[0] + range[1]) * 0.5;
    let midpoint = if ordinary_midpoint.is_finite() {
        ordinary_midpoint
    } else {
        geometric!(cadmpeg_ir::math::interpolate(range[0], range[1], 0.5)).get()
    };
    let scale = [
        geometric!(sketch_geometry_point(&entity.geometry, range[0], ctx)?),
        geometric!(sketch_geometry_point(&entity.geometry, range[1], ctx)?),
        geometric!(sketch_geometry_point(&entity.geometry, midpoint, ctx)?),
    ]
    .into_iter()
    .flat_map(|point| [point.u.abs(), point.v.abs()])
    .fold(1.0_f64, f64::max);
    let target = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    // This bounded polyline supplies tangent order, winding sign, and
    // intersection witnesses only. Exact output retains source parameter
    // intervals rather than this derived representation.
    let count = if travel.is_finite() {
        geometric!(truncate_f64_to_usize(
            (travel / target).ceil().clamp(2.0, 256.0)
        ))
    } else {
        256
    };
    let mut points = Vec::new();
    {
        ctx.reserve_vec(&mut points, count + 1, "f3d profile use polyline")?;
    }
    for index in 0..=count {
        let fraction = geometric!(f64_from_index(index)) / geometric!(f64_from_index(count));
        let ordinary = range[0] + (range[1] - range[0]) * fraction;
        let parameter = if ordinary.is_finite() {
            ordinary
        } else {
            geometric!(cadmpeg_ir::math::interpolate(range[0], range[1], fraction)).get()
        };
        let point = geometric!(sketch_geometry_point(&entity.geometry, parameter, ctx)?);
        points.push(point);
    }
    if reversed {
        points.reverse();
    }
    Ok(Some(points))
}

fn sketch_geometry_speed_bound(
    ctx: &DecodeContext<'_>,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
) -> Result<Option<f64>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    Ok(match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            Some(point_distance(start.get(), end.get()))
        }
        SketchGeometryDefinition::Circle { radius, .. }
        | SketchGeometryDefinition::Arc { radius, .. } => Some(radius.get()),
        SketchGeometryDefinition::Ellipse { radii, .. } => {
            Some(radii.major().get().max(radii.minor().get()))
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            nurbs_speed_bound(ctx, curve)?
        }
        _ if range[0] == range[1] => None,
        _ => None,
    })
}

fn sketch_geometry_point(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    parameter: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Point2>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    Ok(match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some(Point2::new(
            start.u + parameter * (end.u - start.u),
            start.v + parameter * (end.v - start.v),
        )),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => Some(Point2::new(
            center.u + radius.get() * parameter.cos(),
            center.v + radius.get() * parameter.sin(),
        )),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            ..
        } => {
            let (axis_sine, axis_cosine) = major_angle.get().sin_cos();
            Some(Point2::new(
                center.u + radii.major().get() * parameter.cos() * axis_cosine
                    - radii.minor().get() * parameter.sin() * axis_sine,
                center.v
                    + radii.major().get() * parameter.cos() * axis_sine
                    + radii.minor().get() * parameter.sin() * axis_cosine,
            ))
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let lanes = PcurveEvaluatorLanes::new(
                ctx,
                curve.pole_rows(),
                "f3d nurbs evaluator poles",
                "f3d nurbs evaluator weights",
            )?;
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                parameter,
            ))?
            .map(Point2::from)
        }
        _ => None,
    })
}

fn point_on_profile_boundary_use(
    ctx: &DecodeContext<'_>,
    point: Point2,
    use_: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity) else {
        return Ok(false);
    };
    if !point_on_sketch_entity(ctx, point, entity, tolerance)? {
        return Ok(false);
    }
    Ok(match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            let angle = (point.v - center.v).atan2(point.u - center.u);
            directed_angle_parameter(
                angle,
                use_.parameter_range.endpoints()[0],
                use_.parameter_range.endpoints()[1],
            )
            .is_some_and(|parameter| {
                parameter >= -tolerance / radius.get()
                    && parameter <= 1.0 + tolerance / radius.get()
            })
        }
        _ => true,
    })
}

fn signed_polygon_area(vertices: &[Point2]) -> f64 {
    cadmpeg_ir::math::planar::polygon_area_twice(vertices)
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get)
        * 0.5
}

pub(super) fn region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::SketchProfileRegion>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let mut boundaries = Vec::new();
    for profile in &sketch.profiles {
        let Some(boundary) = profile_boundary(profile, entities, tolerance, ctx)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut boundaries, boundary, "f3d profile boundaries")?;
    }
    let mut containment = Vec::new();
    for (outer_index, outer) in boundaries.iter().enumerate() {
        let mut row = Vec::new();
        for (inner_index, inner) in boundaries.iter().enumerate() {
            let contains = outer_index != inner_index && outer.strictly_contains(inner, ctx)?;
            ctx.push_vec(&mut row, contains, "f3d profile containment cell")?;
        }
        ctx.push_vec(&mut containment, row, "f3d profile containment row")?;
    }
    let mut projected = Vec::new();
    for point in points {
        let projected_point = geometric!(project_to_sketch(sketch, *point));
        ctx.push_vec(
            &mut projected,
            projected_point,
            "f3d profile projected point",
        )?;
    }
    let mut incidences = Vec::new();
    for point in &projected {
        let mut incident = HashSet::new();
        for (index, profile) in sketch.profiles.iter().enumerate() {
            for use_ in profile {
                let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity)
                else {
                    continue;
                };
                if point_on_sketch_entity(ctx, *point, entity, tolerance)? {
                    ctx.insert_hash_set(&mut incident, index, "f3d profile incident boundary")?;
                    break;
                }
            }
        }
        ctx.push_vec(&mut incidences, incident, "f3d profile incidence row")?;
    }
    let region = |outer: usize| -> Result<Option<(usize, Vec<usize>)>, CodecError> {
        let holes = immediate_containment_children(outer, &containment, ctx)?;
        Ok(projected
            .iter()
            .zip(&incidences)
            .all(|(point, incident)| {
                incident.contains(&outer)
                    || holes.iter().any(|hole| incident.contains(hole))
                    || incident.is_empty()
                        && boundaries[outer].contains_point(*point)
                        && holes
                            .iter()
                            .all(|hole| !boundaries[*hole].contains_point(*point))
            })
            .then_some((outer, holes)))
    };
    let mut closure_matches = Vec::new();
    for outer in 0..boundaries.len() {
        if let Some(candidate) = region(outer)? {
            ctx.push_vec(&mut closure_matches, candidate, "f3d profile closure match")?;
        }
    }
    if let [(outer, holes)] = closure_matches.as_slice() {
        let mut converted_holes = Vec::new();
        for hole in holes {
            let hole = geometric!(u32::try_from(*hole).ok());
            ctx.push_vec(&mut converted_holes, hole, "f3d profile hole index")?;
        }
        return Ok(SketchProfileRegion::loops(
            geometric!(u32::try_from(*outer).ok()),
            converted_holes,
            ctx,
        )?
        .ok());
    }
    if incidences.iter().any(|incident| !incident.is_empty()) {
        return Ok(None);
    }
    let mut containing = Vec::new();
    for (index, boundary) in boundaries.iter().enumerate() {
        if projected
            .iter()
            .all(|point| boundary.contains_point(*point))
        {
            ctx.push_vec(&mut containing, index, "f3d profile containing boundary")?;
        }
    }
    if containing.iter().enumerate().any(|(left_index, left)| {
        containing
            .iter()
            .skip(left_index + 1)
            .any(|right| !containment[*left][*right] && !containment[*right][*left])
    }) {
        return Ok(None);
    }
    let &outer = geometric!(containing.iter().find(|candidate| {
        containing
            .iter()
            .all(|other| other == *candidate || containment[*other][**candidate])
    }));
    let holes = immediate_containment_children(outer, &containment, ctx)?;
    let mut converted_holes = Vec::new();
    for hole in holes {
        let hole = geometric!(u32::try_from(hole).ok());
        ctx.push_vec(&mut converted_holes, hole, "f3d profile hole index")?;
    }
    Ok(
        SketchProfileRegion::loops(geometric!(u32::try_from(outer).ok()), converted_holes, ctx)?
            .ok(),
    )
}

/// Return true when every selected closed profile bounds a disjoint region.
/// Nested or intersecting loops require explicit region semantics.
pub(super) fn profile_loops_are_independent(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    profiles: &[u32],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut boundaries = Vec::new();
    for profile in profiles {
        let Ok(index) = usize::try_from(*profile) else {
            return Ok(false);
        };
        let Some(profile) = sketch.profiles.get(index) else {
            return Ok(false);
        };
        let Some(boundary) = profile_boundary(profile, entities, tolerance, ctx)? else {
            return Ok(false);
        };
        ctx.push_vec(
            &mut boundaries,
            boundary,
            "f3d independent profile boundaries",
        )?;
    }
    for (left_index, left) in boundaries.iter().enumerate() {
        for right in boundaries.iter().skip(left_index + 1) {
            if !left.is_provably_disjoint(right, ctx)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn immediate_containment_children(
    outer: usize,
    containment: &[Vec<bool>],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<usize>, CodecError> {
    let mut children = Vec::new();
    for candidate in 0..containment.len() {
        if candidate != outer
            && containment[outer][candidate]
            && !(0..containment.len()).any(|intermediate| {
                intermediate != outer
                    && intermediate != candidate
                    && containment[outer][intermediate]
                    && containment[intermediate][candidate]
            })
        {
            ctx.push_vec(&mut children, candidate, "f3d profile immediate hole")?;
        }
    }
    Ok(children)
}

enum ProfileBoundary {
    Polygon(Vec<Point2>),
    CircularArcLoop(Vec<ProfileBoundarySegment>),
    Circle {
        center: Point2,
        radius: PositiveLength,
    },
    CertifiedLoop(CertifiedProfileLoop),
}

#[derive(Clone)]
struct CertifiedProfileLoop {
    tubes: Vec<CertifiedCurveTube>,
}

#[derive(Clone)]
struct CertifiedCurveTube {
    start: Point2,
    end: Point2,
    error: f64,
}

enum ProfileBoundarySegment {
    Line {
        start: Point2,
        end: Point2,
    },
    Arc {
        center: Point2,
        radius: PositiveLength,
        start_angle: f64,
        end_angle: f64,
    },
}

impl ProfileBoundary {
    fn contains_point(&self, point: Point2) -> bool {
        match self {
            Self::Polygon(vertices) => point_in_polygon(point, vertices),
            Self::CircularArcLoop(segments) => point_in_circular_arc_loop(point, segments),
            Self::Circle { center, radius } => point_distance(*center, point) < radius.get(),
            Self::CertifiedLoop(loop_) => loop_.contains_point(point),
        }
    }

    fn strictly_contains(&self, inner: &Self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        Ok(match (self, inner) {
            (Self::Polygon(outer), Self::Polygon(inner)) => polygon_strictly_contains(outer, inner),
            (
                Self::Circle {
                    center: outer_center,
                    radius: outer_radius,
                },
                Self::Circle {
                    center: inner_center,
                    radius: inner_radius,
                },
            ) => {
                point_distance(*outer_center, *inner_center) + inner_radius.get()
                    < outer_radius.get()
            }
            (Self::Polygon(outer), Self::Circle { center, radius }) => {
                point_in_polygon(*center, outer)
                    && polygon_edges(outer)
                        .all(|edge| point_segment_distance(*center, edge) > radius.get())
            }
            (Self::CircularArcLoop(outer), Self::Circle { center, radius }) => {
                point_in_circular_arc_loop(*center, outer)
                    && outer.iter().all(|segment| {
                        point_boundary_segment_distance(*center, segment) > radius.get()
                    })
            }
            (Self::Circle { center, radius }, Self::Polygon(inner)) => inner
                .iter()
                .all(|point| point_distance(*center, *point) < radius.get()),
            (Self::Circle { center, radius }, Self::CircularArcLoop(inner)) => inner
                .iter()
                .all(|segment| boundary_segment_max_distance(*center, segment) < radius.get()),
            (Self::Polygon(outer), Self::CircularArcLoop(inner)) => {
                !polygon_arc_loop_intersects(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_polygon(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (Self::CircularArcLoop(outer), Self::Polygon(inner)) => {
                !polygon_arc_loop_intersects(inner, outer)
                    && inner
                        .first()
                        .is_some_and(|point| point_in_circular_arc_loop(*point, outer))
            }
            (Self::CircularArcLoop(outer), Self::CircularArcLoop(inner)) => {
                !arc_loops_intersect(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_circular_arc_loop(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (outer, inner) => {
                let Some(outer) = outer.certified_loop(ctx)? else {
                    return Ok(false);
                };
                let Some(inner) = inner.certified_loop(ctx)? else {
                    return Ok(false);
                };
                outer.strictly_contains(&inner)
            }
        })
    }

    fn is_provably_disjoint(
        &self,
        other: &Self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        let intersects = match (self, other) {
            (Self::Polygon(left), Self::Polygon(right)) => polygon_edges(left).any(|left_edge| {
                polygon_edges(right).any(|right_edge| segments_intersect(left_edge, right_edge))
            }),
            (
                Self::Circle {
                    center: left_center,
                    radius: left_radius,
                },
                Self::Circle {
                    center: right_center,
                    radius: right_radius,
                },
            ) => {
                point_distance(*left_center, *right_center)
                    <= left_radius.get() + right_radius.get()
            }
            (Self::Polygon(polygon), Self::Circle { center, radius })
            | (Self::Circle { center, radius }, Self::Polygon(polygon)) => polygon_edges(polygon)
                .any(|edge| point_segment_distance(*center, edge) <= radius.get()),
            (Self::Polygon(polygon), Self::CircularArcLoop(arc_loop))
            | (Self::CircularArcLoop(arc_loop), Self::Polygon(polygon)) => {
                polygon_arc_loop_intersects(polygon, arc_loop)
            }
            (Self::CircularArcLoop(left), Self::CircularArcLoop(right)) => {
                arc_loops_intersect(left, right)
            }
            (Self::CircularArcLoop(arc_loop), Self::Circle { center, radius })
            | (Self::Circle { center, radius }, Self::CircularArcLoop(arc_loop)) => arc_loop
                .iter()
                .any(|segment| point_boundary_segment_distance(*center, segment) <= radius.get()),
            (Self::CertifiedLoop(_), _) | (_, Self::CertifiedLoop(_)) => return Ok(false),
        };
        Ok(!intersects
            && !self.strictly_contains(other, ctx)?
            && !other.strictly_contains(self, ctx)?)
    }

    fn certified_loop(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<std::borrow::Cow<'_, CertifiedProfileLoop>>, CodecError> {
        use std::borrow::Cow;

        Ok(match self {
            Self::Polygon(vertices) => {
                CertifiedProfileLoop::from_vertices(vertices, ctx)?.map(Cow::Owned)
            }
            Self::CircularArcLoop(segments) => {
                certified_analytic_loop(segments, ctx)?.map(Cow::Owned)
            }
            Self::Circle { center, radius } => {
                certified_circle(*center, *radius, ctx)?.map(Cow::Owned)
            }
            Self::CertifiedLoop(loop_) => Some(Cow::Borrowed(loop_)),
        })
    }
}

impl CertifiedProfileLoop {
    fn new(tubes: Vec<CertifiedCurveTube>) -> Option<Self> {
        (tubes.len() >= 3).then_some(Self { tubes })
    }

    fn vertices(&self) -> impl Iterator<Item = Point2> + Clone + '_ {
        self.tubes.iter().map(|tube| tube.start)
    }

    fn from_vertices(
        vertices: &[Point2],
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Self>, CodecError> {
        let mut tubes = Vec::new();
        for (start, end) in polygon_edges(vertices) {
            ctx.push_vec(
                &mut tubes,
                CertifiedCurveTube {
                    start,
                    end,
                    error: 0.0,
                },
                "f3d certified polygon tube",
            )?;
        }
        Ok(Self::new(tubes))
    }

    fn contains_point(&self, point: Point2) -> bool {
        self.tubes
            .iter()
            .all(|tube| point_segment_distance(point, (tube.start, tube.end)) > tube.error)
            && point_in_polygon_edges(
                point,
                self.vertices()
                    .zip(self.vertices().cycle().skip(1))
                    .take(self.tubes.len()),
            )
    }

    fn strictly_contains(&self, inner: &Self) -> bool {
        self.tubes.iter().all(|outer| {
            inner.tubes.iter().all(|inner| {
                segment_distance((outer.start, outer.end), (inner.start, inner.end))
                    > outer.error + inner.error
            })
        }) && inner
            .vertices()
            .next()
            .is_some_and(|point| self.contains_point(point))
    }
}

fn profile_boundary(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ProfileBoundary>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let [use_] = profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        if let SketchGeometryDefinition::Circle { center, radius } = *entity.geometry.definition() {
            return Ok(Some(ProfileBoundary::Circle {
                center: center.get(),
                radius,
            }));
        }
    }
    if let Some(polygon) = line_profile_vertices(profile, entities, tolerance, ctx)? {
        return Ok(Some(ProfileBoundary::Polygon(polygon)));
    }
    if let Some(arc_loop) = circular_arc_profile_segments(profile, entities, tolerance, ctx)? {
        return Ok(Some(ProfileBoundary::CircularArcLoop(arc_loop)));
    }
    Ok(certified_profile_loop(profile, entities, tolerance, ctx)?
        .map(ProfileBoundary::CertifiedLoop))
}

fn certified_profile_loop(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CertifiedProfileLoop>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut scale = 1.0_f64;
    for entity in entities {
        if let Some(ends) = sketch_entity_endpoints(entity, ctx)? {
            for point in ends {
                scale = scale.max(point.u.abs()).max(point.v.abs());
            }
        }
    }
    // The tube need only separate the selected point and peer boundaries; it
    // is not a geometric approximation exposed by the codec.  A square-root
    // scale keeps the conservative tube practical while exact boundary tests
    // continue to govern the source linear tolerance.
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    let mut tubes = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        let mut entity_tubes = match entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => vec![CertifiedCurveTube {
                start: start.get(),
                end: end.get(),
                error: 0.0,
            }],
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => geometric!(certified_arc_tubes(
                center.get(),
                *radius,
                start_angle.get(),
                end_angle.get(),
                target_error,
                ctx,
            )?),
            SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
                geometric!(certified_nurbs_tubes(curve, target_error, ctx)?)
            }
            _ => return Ok(None),
        };
        if use_.reversed {
            entity_tubes.reverse();
            for tube in &mut entity_tubes {
                std::mem::swap(&mut tube.start, &mut tube.end);
            }
        }
        let first = geometric!(entity_tubes.first()).start;
        if previous_end.is_some_and(|end| point_distance(end, first) > tolerance) {
            return Ok(None);
        }
        previous_end = entity_tubes.last().map(|tube| tube.end);
        {
            ctx.reserve_vec(
                &mut tubes,
                entity_tubes.len(),
                "f3d certified profile tubes",
            )?;
        }
        tubes.extend(entity_tubes);
    }
    if previous_end.is_none_or(|end| {
        tubes
            .first()
            .is_none_or(|first| point_distance(end, first.start) > tolerance)
    }) {
        return Ok(None);
    }
    Ok(CertifiedProfileLoop::new(tubes))
}

fn certified_analytic_loop(
    segments: &[ProfileBoundarySegment],
    ctx: &DecodeContext<'_>,
) -> Result<Option<CertifiedProfileLoop>, CodecError> {
    let scale = segments
        .iter()
        .flat_map(|segment| {
            let (start, end) = boundary_segment_endpoints(segment);
            [start.u.abs(), start.v.abs(), end.u.abs(), end.v.abs()]
        })
        .fold(1.0_f64, f64::max);
    let tolerance = EPS_GEOMETRY_CERTIFIED_ANALYTIC_LOOP_E6 * scale;
    let mut tubes = Vec::new();
    for segment in segments {
        let segment_tubes = match segment {
            ProfileBoundarySegment::Line { start, end } => vec![CertifiedCurveTube {
                start: *start,
                end: *end,
                error: 0.0,
            }],
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => geometric!(certified_arc_tubes(
                *center,
                *radius,
                *start_angle,
                *end_angle,
                tolerance,
                ctx
            )?),
        };
        for tube in segment_tubes {
            ctx.push_vec(&mut tubes, tube, "f3d certified analytic tube")?;
        }
    }
    Ok(CertifiedProfileLoop::new(tubes))
}

fn certified_circle(
    center: Point2,
    radius: PositiveLength,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CertifiedProfileLoop>, CodecError> {
    let tolerance = EPS_GEOMETRY_CERTIFIED_CIRCLE_E6
        * (1.0 + center.u.abs().max(center.v.abs()).max(radius.get()));
    let tubes = geometric!(certified_arc_tubes(
        center,
        radius,
        0.0,
        std::f64::consts::TAU,
        tolerance,
        ctx
    )?);
    Ok(CertifiedProfileLoop::new(tubes))
}

fn certified_arc_tubes(
    center: Point2,
    radius: PositiveLength,
    start: f64,
    end: f64,
    target_error: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<CertifiedCurveTube>>, CodecError> {
    let sweep = end - start;
    if !sweep.is_finite() || sweep == 0.0 {
        return Ok(None);
    }
    let radius = radius.get();
    let count = geometric!(subdivision_count(radius * sweep.abs(), target_error));
    let count_float = geometric!(f64_from_index(count));
    let error = radius * sweep.abs() / count_float;
    let mut tubes = Vec::new();
    {
        ctx.reserve_vec(&mut tubes, count, "f3d certified arc tubes")?;
    }
    for index in 0..count {
        let parameter = |ordinal: usize| {
            f64_from_index(ordinal).map(|ordinal| start + sweep * ordinal / count_float)
        };
        let point = |angle: f64| {
            Point2::new(
                center.u + radius * angle.cos(),
                center.v + radius * angle.sin(),
            )
        };
        tubes.push(CertifiedCurveTube {
            start: point(geometric!(parameter(index))),
            end: point(geometric!(parameter(index + 1))),
            error,
        });
    }
    Ok(Some(tubes))
}

fn certified_nurbs_tubes(
    curve: &PcurveNurbs,
    target_error: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<CertifiedCurveTube>>, CodecError> {
    let speed = geometric!(nurbs_speed_bound(ctx, curve)?);
    let degree = index_from_u32(curve.degree());
    let knots = curve.knots();
    let lanes = PcurveEvaluatorLanes::new(
        ctx,
        curve.pole_rows(),
        "f3d nurbs tube points",
        "f3d nurbs tube weights",
    )?;
    let count = lanes.points().len();
    let mut tubes = Vec::new();
    for span in knots[degree..=count].windows(2) {
        if span[0] == span[1] {
            continue;
        }
        let width = span[1] - span[0];
        let travel_bound = if width.is_finite() {
            speed * width
        } else if speed <= 1.0 {
            speed * span[1] - speed * span[0]
        } else {
            return Ok(None);
        };
        let subdivisions = geometric!(subdivision_count(travel_bound, target_error));
        let subdivisions_float = geometric!(f64_from_index(subdivisions));
        let error = travel_bound / subdivisions_float;
        for index in 0..subdivisions {
            let parameter = |ordinal: usize| {
                let ordinal = f64_from_index(ordinal)?;
                let fraction = ordinal / subdivisions_float;
                if width.is_finite() {
                    Some(span[0] + width * ordinal / subdivisions_float)
                } else {
                    Some(span[0].mul_add(1.0 - fraction, span[1] * fraction))
                }
            };
            let start = *geometric!(cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    ctx,
                    curve.degree(),
                    knots,
                    lanes.points(),
                    lanes.weights(),
                    geometric!(parameter(index)),
                )
            )?)
            .as_raw();
            let end = *geometric!(cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    ctx,
                    curve.degree(),
                    knots,
                    lanes.points(),
                    lanes.weights(),
                    geometric!(parameter(index + 1)),
                )
            )?)
            .as_raw();
            {
                ctx.reserve_vec(&mut tubes, 1, "f3d certified nurbs tube")?;
            }
            tubes.push(CertifiedCurveTube { start, end, error });
        }
    }
    Ok((!tubes.is_empty()).then_some(tubes))
}

fn subdivision_count(travel_bound: f64, target_error: f64) -> Option<usize> {
    // Format invariant: densification above this count is outside the reader.
    // Session work for arrangement walks is charged separately via work_budget.
    const MAX_SUBDIVISIONS: f64 = 100_000.0;
    if !travel_bound.is_finite()
        || travel_bound < 0.0
        || !target_error.is_finite()
        || target_error <= 0.0
    {
        return None;
    }
    let count = (travel_bound / target_error).ceil().max(1.0);
    (count <= MAX_SUBDIVISIONS)
        .then(|| truncate_f64_to_usize(count))
        .flatten()
}

fn nurbs_speed_bound(
    ctx: &DecodeContext<'_>,
    curve: &PcurveNurbs,
) -> Result<Option<f64>, CodecError> {
    Ok(cadmpeg_ir::geometry::nurbs::bounds::speed_bound_by(
        ctx,
        curve.degree(),
        curve.knots(),
        curve.pole_rows().count(),
        |index| {
            curve.pole_rows().point_at(index).map(|point| {
                let point = point.get();
                [point.u, point.v]
            })
        },
        |index| curve.pole_rows().weight_at(index).unwrap_or(1.0),
        [0.0, 0.0],
    )?
    .map(cadmpeg_ir::scalar::FiniteReal::get))
}

fn circular_arc_profile_segments(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<ProfileBoundarySegment>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut segments = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        let segment = match *entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                let [start, end] = if use_.reversed {
                    [end, start]
                } else {
                    [start, end]
                };
                ProfileBoundarySegment::Line {
                    start: start.get(),
                    end: end.get(),
                }
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let (start_angle, end_angle) = if use_.reversed {
                    (end_angle.get(), start_angle.get())
                } else {
                    (start_angle.get(), end_angle.get())
                };
                ProfileBoundarySegment::Arc {
                    center: center.get(),
                    radius,
                    start_angle,
                    end_angle,
                }
            }
            _ => return Ok(None),
        };
        let (start, end) = boundary_segment_endpoints(&segment);
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return Ok(None);
        }
        previous_end = Some(end);
        ctx.push_vec(&mut segments, segment, "f3d circular arc profile segment")?;
    }
    let first_start = geometric!(segments.first().map(boundary_segment_endpoints)).0;
    Ok((segments.len() >= 2
        && previous_end.is_some_and(|end| point_distance(end, first_start) <= tolerance))
    .then_some(segments))
}

fn line_profile_vertices(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point2>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut vertices = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        let SketchGeometryDefinition::Line { start, end } = *entity.geometry.definition() else {
            return Ok(None);
        };
        let (start, end) = (start.get(), end.get());
        let [start, end] = if use_.reversed {
            [end, start]
        } else {
            [start, end]
        };
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return Ok(None);
        }
        ctx.push_vec(&mut vertices, start, "f3d line profile vertex")?;
        previous_end = Some(end);
    }
    if vertices.len() < 3
        || previous_end.is_none_or(|end| point_distance(end, vertices[0]) > tolerance)
    {
        return Ok(None);
    }
    Ok(Some(vertices))
}

fn boundary_segment_endpoints(segment: &ProfileBoundarySegment) -> (Point2, Point2) {
    match segment {
        ProfileBoundarySegment::Line { start, end } => (*start, *end),
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radius = radius.get();
            (
                Point2::new(
                    center.u + radius * start_angle.cos(),
                    center.v + radius * start_angle.sin(),
                ),
                Point2::new(
                    center.u + radius * end_angle.cos(),
                    center.v + radius * end_angle.sin(),
                ),
            )
        }
    }
}

fn point_in_circular_arc_loop(point: Point2, segments: &[ProfileBoundarySegment]) -> bool {
    segments
        .iter()
        .map(|segment| match segment {
            ProfileBoundarySegment::Line { start, end } => {
                let crosses_up = start.v <= point.v && point.v < end.v;
                let crosses_down = end.v <= point.v && point.v < start.v;
                if !(crosses_up || crosses_down)
                    || point.u
                        >= start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
                {
                    0
                } else if crosses_up {
                    1
                } else {
                    -1
                }
            }
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => horizontal_ray_arc_winding(point, *center, radius.get(), *start_angle, *end_angle),
        })
        .sum::<i32>()
        != 0
}

fn horizontal_ray_arc_winding(
    point: Point2,
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> i32 {
    let ordinate = (point.v - center.v) / radius;
    if ordinate.abs() > 1.0 {
        return 0;
    }
    let principal = ordinate.asin();
    let sweep = end_angle - start_angle;
    let angles = if principal.cos().abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
        [Some(principal), None]
    } else {
        [Some(principal), Some(std::f64::consts::PI - principal)]
    };
    angles
        .into_iter()
        .flatten()
        .filter(|angle| center.u + radius * angle.cos() > point.u)
        .filter_map(|angle| {
            let parameter = directed_angle_parameter(angle, start_angle, end_angle)?;
            let derivative = radius * angle.cos() * sweep;
            let second_derivative = -radius * angle.sin() * sweep * sweep;
            let direction = if parameter <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
                && derivative.abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
            {
                second_derivative
            } else if parameter >= 1.0 - EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
                && derivative.abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
            {
                -second_derivative
            } else {
                derivative
            };
            if parameter <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                (direction > 0.0).then_some(1)
            } else if parameter >= 1.0 - EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                (direction < 0.0).then_some(-1)
            } else if direction > EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                Some(1)
            } else if direction < -EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                Some(-1)
            } else {
                None
            }
        })
        .sum()
}

fn directed_angle_parameter(angle: f64, start: f64, end: f64) -> Option<f64> {
    let sweep = end - start;
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU {
        return None;
    }
    let displacement = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        -(start - angle).rem_euclid(std::f64::consts::TAU)
    };
    (displacement.abs() <= sweep.abs()).then_some(displacement / sweep)
}

fn point_boundary_segment_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    match segment {
        ProfileBoundarySegment::Line { start, end } => {
            point_segment_distance(point, (*start, *end))
        }
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radius = radius.get();
            let endpoint_distance = [*start_angle, *end_angle]
                .into_iter()
                .map(|angle| {
                    point_distance(
                        point,
                        Point2::new(
                            center.u + radius * angle.cos(),
                            center.v + radius * angle.sin(),
                        ),
                    )
                })
                .fold(f64::INFINITY, f64::min);
            let angle = (point.v - center.v).atan2(point.u - center.u);
            if directed_angle_parameter(angle, *start_angle, *end_angle).is_some() {
                endpoint_distance.min((point_distance(point, *center) - radius).abs())
            } else {
                endpoint_distance
            }
        }
    }
}

fn boundary_segment_max_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    let (start, end) = boundary_segment_endpoints(segment);
    let endpoint_distance = point_distance(point, start).max(point_distance(point, end));
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = segment
    else {
        return endpoint_distance;
    };
    let radius = radius.get();
    let farthest_angle = (point.v - center.v).atan2(point.u - center.u) + std::f64::consts::PI;
    if directed_angle_parameter(farthest_angle, *start_angle, *end_angle).is_some() {
        endpoint_distance.max(point_distance(point, *center) + radius)
    } else {
        endpoint_distance
    }
}

pub(super) fn point_in_polygon(point: Point2, vertices: &[Point2]) -> bool {
    point_in_polygon_edges(point, polygon_edges(vertices))
}

fn point_in_polygon_edges(point: Point2, edges: impl Iterator<Item = (Point2, Point2)>) -> bool {
    edges
        .filter(|(start, end)| {
            (start.v > point.v) != (end.v > point.v)
                && point.u < start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
        })
        .count()
        % 2
        == 1
}

fn polygon_strictly_contains(outer: &[Point2], inner: &[Point2]) -> bool {
    inner.iter().all(|point| point_in_polygon(*point, outer))
        && !polygon_edges(outer).any(|outer_edge| {
            polygon_edges(inner).any(|inner_edge| segments_intersect(outer_edge, inner_edge))
        })
}

fn polygon_edges(vertices: &[Point2]) -> impl Iterator<Item = (Point2, Point2)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

/// Answer the distance from `point` to the segment `start`-`end`.
///
/// The segment carries the parameter interval `[0, 1]`, so restricting the
/// projection parameter to that interval is the segment's own definition
/// rather than a correction. A projection outside the interval is not an
/// error: it states that the point projects onto the supporting line beyond an
/// end, and the nearest point of the segment is then that end.
pub(super) fn point_segment_distance(point: Point2, (start, end): (Point2, Point2)) -> f64 {
    let Some(parameter) = cadmpeg_ir::math::planar::line_projection_parameter(start, end, point)
    else {
        return point_distance(point, start).min(point_distance(point, end));
    };
    let parameter = parameter.get().clamp(0.0, 1.0);
    let (Some(u), Some(v)) = (
        cadmpeg_ir::math::interpolate(start.u, end.u, parameter),
        cadmpeg_ir::math::interpolate(start.v, end.v, parameter),
    ) else {
        return f64::INFINITY;
    };
    point_distance(point, Point2::new(u.get(), v.get()))
}

fn segment_distance(left: (Point2, Point2), right: (Point2, Point2)) -> f64 {
    if segments_intersect(left, right) {
        0.0
    } else {
        [
            point_segment_distance(left.0, right),
            point_segment_distance(left.1, right),
            point_segment_distance(right.0, left),
            point_segment_distance(right.1, left),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
    }
}

fn segments_intersect(left: (Point2, Point2), right: (Point2, Point2)) -> bool {
    use cadmpeg_ir::math::planar::orientation;
    use std::cmp::Ordering::Equal;
    let overlaps = |a: f64, b: f64, c: f64, d: f64| a.min(b) <= c.max(d) && c.min(d) <= a.max(b);
    if !overlaps(left.0.u, left.1.u, right.0.u, right.1.u)
        || !overlaps(left.0.v, left.1.v, right.0.v, right.1.v)
    {
        return false;
    }
    let [Some(a), Some(b), Some(c), Some(d)] = [
        orientation(left.0, left.1, right.0),
        orientation(left.0, left.1, right.1),
        orientation(right.0, right.1, left.0),
        orientation(right.0, right.1, left.1),
    ] else {
        return false;
    };
    (a == Equal || b == Equal || a != b) && (c == Equal || d == Equal || c != d)
}

fn polygon_arc_loop_intersects(polygon: &[Point2], arc_loop: &[ProfileBoundarySegment]) -> bool {
    polygon_edges(polygon).any(|(start, end)| {
        let line = ProfileBoundarySegment::Line { start, end };
        arc_loop
            .iter()
            .any(|segment| boundary_segments_intersect(&line, segment))
    })
}

fn arc_loops_intersect(left: &[ProfileBoundarySegment], right: &[ProfileBoundarySegment]) -> bool {
    left.iter().any(|left| {
        right
            .iter()
            .any(|right| boundary_segments_intersect(left, right))
    })
}

fn boundary_segments_intersect(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> bool {
    match (left, right) {
        (
            ProfileBoundarySegment::Line {
                start: left_start,
                end: left_end,
            },
            ProfileBoundarySegment::Line {
                start: right_start,
                end: right_end,
            },
        ) => segments_intersect((*left_start, *left_end), (*right_start, *right_end)),
        (ProfileBoundarySegment::Line { start, end }, arc @ ProfileBoundarySegment::Arc { .. })
        | (arc @ ProfileBoundarySegment::Arc { .. }, ProfileBoundarySegment::Line { start, end }) => {
            line_arc_intersection_points((*start, *end), arc)
                .is_some_and(|points| points.iter().any(Option::is_some))
        }
        (
            ProfileBoundarySegment::Arc {
                center: left_center,
                radius: left_radius,
                start_angle: left_start,
                end_angle: left_end,
            },
            ProfileBoundarySegment::Arc {
                center: right_center,
                radius: right_radius,
                start_angle: right_start,
                end_angle: right_end,
            },
        ) => arcs_intersect(
            (*left_center, *left_radius, *left_start, *left_end),
            (*right_center, *right_radius, *right_start, *right_end),
        ),
    }
}

fn arcs_intersect(
    left: (Point2, PositiveLength, f64, f64),
    right: (Point2, PositiveLength, f64, f64),
) -> bool {
    let (left_center, left_radius, left_start, left_end) = left;
    let (right_center, right_radius, right_start, right_end) = right;
    if left_center == right_center && left_radius == right_radius {
        return [left_start, left_end]
            .into_iter()
            .any(|angle| directed_angle_parameter(angle, right_start, right_end).is_some())
            || [right_start, right_end]
                .into_iter()
                .any(|angle| directed_angle_parameter(angle, left_start, left_end).is_some());
    }
    let left = ProfileBoundarySegment::Arc {
        center: left_center,
        radius: left_radius,
        start_angle: left_start,
        end_angle: left_end,
    };
    let right = ProfileBoundarySegment::Arc {
        center: right_center,
        radius: right_radius,
        start_angle: right_start,
        end_angle: right_end,
    };
    arc_intersection_points(&left, &right).is_some_and(|points| points.iter().any(Option::is_some))
}

pub(super) fn historical_member_points_in_state(
    member: &DesignExtrudeSelectionMember,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let (kind, entity_ref) = match &member.historical {
        Some(binding) => (binding.kind, binding.entity_ref),
        None => {
            let Some(geometry) = member.resolved_geometry.as_ref() else {
                return Ok(None);
            };
            let kind = match geometry {
                SketchRelationOperand::Point { .. } => AsmHistoricalEntityKind::Point,
                SketchRelationOperand::Curve { .. } => AsmHistoricalEntityKind::Curve,
                SketchRelationOperand::Surface { .. } | SketchRelationOperand::Record { .. } => {
                    return Ok(None);
                }
            };
            let Ok(entity_ref) = i64::try_from(member.local_id) else {
                return Ok(None);
            };
            (kind, entity_ref)
        }
    };
    historical_entity_positions(kind, entity_ref, topology, ctx)
}

fn historical_entity_positions(
    kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let mut positions = Vec::new();
    let mut edge_refs = Vec::new();
    match kind {
        AsmHistoricalEntityKind::Coedge => {
            for coedge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.coedge == local_id)
            {
                ctx.push_vec(
                    &mut edge_refs,
                    coedge.edge,
                    "f3d historical coedge edge reference",
                )?;
            }
        }
        AsmHistoricalEntityKind::Edge => {
            ctx.push_vec(
                &mut edge_refs,
                local_id,
                "f3d historical direct edge reference",
            )?;
        }
        AsmHistoricalEntityKind::Curve => {
            for binding in topology
                .edge_curves
                .iter()
                .filter(|binding| binding.carrier == Some(local_id))
            {
                ctx.push_vec(
                    &mut edge_refs,
                    binding.entity,
                    "f3d historical curve edge reference",
                )?;
            }
        }
        AsmHistoricalEntityKind::Loop => {
            for coedge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.owner_loop == local_id)
            {
                ctx.push_vec(
                    &mut edge_refs,
                    coedge.edge,
                    "f3d historical loop edge reference",
                )?;
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            for binding in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(local_id))
            {
                if let Some(coedge) = topology
                    .coedge_topology
                    .iter()
                    .find(|coedge| coedge.coedge == binding.entity)
                {
                    ctx.push_vec(
                        &mut edge_refs,
                        coedge.edge,
                        "f3d historical pcurve edge reference",
                    )?;
                }
            }
        }
        AsmHistoricalEntityKind::Vertex => {
            for point in historical_vertex_positions(topology, local_id) {
                ctx.push_vec(&mut positions, point, "f3d historical vertex position")?;
            }
        }
        AsmHistoricalEntityKind::Point => {
            for point in topology
                .point_positions
                .iter()
                .filter(|point| point.point == local_id)
            {
                ctx.push_vec(
                    &mut positions,
                    point.position,
                    "f3d historical point position",
                )?;
            }
        }
        AsmHistoricalEntityKind::Face => {
            let Some(points) = historical_face_points(local_id, topology, ctx)? else {
                return Ok(None);
            };
            for point in points {
                ctx.push_vec(&mut positions, point, "f3d historical face position")?;
            }
        }
        AsmHistoricalEntityKind::Surface => {
            let mut found = false;
            for binding in topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == local_id)
            {
                found = true;
                let Some(points) = historical_face_points(binding.entity, topology, ctx)? else {
                    return Ok(None);
                };
                for point in points {
                    ctx.push_vec(&mut positions, point, "f3d historical surface position")?;
                }
            }
            if !found {
                return Ok(None);
            }
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell => {
            let Some(faces) = historical_owned_faces(kind, local_id, topology, ctx)? else {
                return Ok(None);
            };
            for face in faces {
                let Some(points) = historical_face_points(face, topology, ctx)? else {
                    return Ok(None);
                };
                for point in points {
                    ctx.push_vec(&mut positions, point, "f3d historical owned face position")?;
                }
            }
        }
    }
    for edge_ref in edge_refs {
        let Some(edge) = topology
            .edge_vertices
            .iter()
            .find(|edge| edge.edge == edge_ref)
        else {
            return Ok(None);
        };
        let mut start = historical_vertex_positions(topology, edge.start_vertex).peekable();
        let mut end = historical_vertex_positions(topology, edge.end_vertex).peekable();
        if start.peek().is_none() || end.peek().is_none() {
            return Ok(None);
        }
        for point in start.chain(end) {
            ctx.push_vec(&mut positions, point, "f3d historical edge position")?;
        }
    }
    Ok((!positions.is_empty()).then_some(positions))
}

fn historical_owned_faces(
    kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let mut faces = Vec::new();
    match kind {
        AsmHistoricalEntityKind::Body => {
            let Some(regions) = historical_relation_members(&topology.body_regions, local_id)
            else {
                return Ok(None);
            };
            for region in regions {
                let Some(shells) = historical_relation_members(&topology.region_shells, *region)
                else {
                    return Ok(None);
                };
                for shell in shells {
                    if !append_historical_shell_faces(*shell, topology, ctx, &mut faces)? {
                        return Ok(None);
                    }
                }
            }
        }
        AsmHistoricalEntityKind::Region => {
            let Some(shells) = historical_relation_members(&topology.region_shells, local_id)
            else {
                return Ok(None);
            };
            for shell in shells {
                if !append_historical_shell_faces(*shell, topology, ctx, &mut faces)? {
                    return Ok(None);
                }
            }
        }
        AsmHistoricalEntityKind::Shell => {
            if !append_historical_shell_faces(local_id, topology, ctx, &mut faces)? {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    }
    ctx.sort_unstable_by(
        &mut faces,
        Ord::cmp,
        |_| 0,
        "sort F3D historical owned faces",
    )?;
    faces.dedup();
    Ok((!faces.is_empty()).then_some(faces))
}

fn historical_relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = &matches.next()?.member_refs;
    matches.next().is_none().then_some(members)
}

fn append_historical_shell_faces(
    shell: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
    faces: &mut Vec<i64>,
) -> Result<bool, CodecError> {
    let Some(members) = historical_relation_members(&topology.shell_faces, shell) else {
        return Ok(false);
    };
    for face in members {
        ctx.push_vec(faces, *face, "f3d historical owned face")?;
    }
    Ok(true)
}

fn historical_vertex_positions(
    topology: &crate::history_records::AsmHistoricalTopology,
    vertex_ref: i64,
) -> impl Iterator<Item = Point3> + '_ {
    topology
        .vertex_points
        .iter()
        .filter(move |binding| binding.entity == vertex_ref)
        .filter_map(|binding| {
            topology
                .point_positions
                .iter()
                .find(|point| point.point == binding.carrier)
                .map(|point| point.position)
        })
}

pub(super) fn project_to_sketch(
    sketch: &cadmpeg_ir::sketches::Sketch,
    point: Point3,
) -> Option<Point2> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let offset = point.vector_from(origin.get());
    let v_axis = normal.cross(u_axis.get());
    Some(Point2::new(offset.dot(u_axis.get()), offset.dot(v_axis)))
}

pub(super) fn point_on_sketch_entity(
    ctx: &DecodeContext<'_>,
    point: Point2,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    tolerance: f64,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() {
        if curve.periodic() {
            return Ok(false);
        }
        let lanes = PcurveEvaluatorLanes::new(
            ctx,
            curve.pole_rows(),
            "f3d nurbs evaluator poles",
            "f3d nurbs evaluator weights",
        )?;
        return cadmpeg_ir::eval::nurbs_pcurve_contains_point(
            ctx,
            curve.degree(),
            curve.knots(),
            lanes.points(),
            lanes.weights(),
            point,
            tolerance,
        )
        .map(|contained| contained.unwrap_or(false))
        .map_err(CodecError::ResourceLimit);
    }
    Ok((|| match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let dx = end.u - start.u;
            let dy = end.v - start.v;
            let length_squared = dx * dx + dy * dy;
            if length_squared == 0.0 {
                return point_distance(point, start.get()) <= tolerance;
            }
            let t = ((point.u - start.u) * dx + (point.v - start.v) * dy) / length_squared;
            if !(-tolerance..=1.0 + tolerance).contains(&t) {
                return false;
            }
            point_distance(point, Point2::new(start.u + t * dx, start.v + t * dy)) <= tolerance
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            (point_distance(point, center.get()) - radius.get()).abs() <= tolerance
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radial_error = (point_distance(point, center.get()) - radius.get()).abs();
            if radial_error > tolerance {
                return false;
            }
            let angle = (point.v - center.v).atan2(point.u - center.u);
            angle_in_sweep(
                angle,
                start_angle.get(),
                end_angle.get(),
                tolerance / radius.get(),
            )
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let local_u = du * cosine + dv * sine;
            let local_v = -du * sine + dv * cosine;
            let parameter = (local_v / radii.minor().get()).atan2(local_u / radii.major().get());
            let boundary = Point2::new(
                center.u + radii.major().get() * parameter.cos() * cosine
                    - radii.minor().get() * parameter.sin() * sine,
                center.v
                    + radii.major().get() * parameter.cos() * sine
                    + radii.minor().get() * parameter.sin() * cosine,
            );
            if point_distance(point, boundary) > tolerance {
                return false;
            }
            match bounds {
                None => true,
                Some([start, end]) => angle_in_sweep(
                    parameter,
                    start.get(),
                    end.get(),
                    tolerance / radii.major().get().min(radii.minor().get()),
                ),
            }
        }
        _ => false,
    })())
}

pub(super) fn angle_in_sweep(angle: f64, start: f64, end: f64, tolerance: f64) -> bool {
    let sweep = end - start;
    if sweep.abs() >= std::f64::consts::TAU - tolerance {
        return true;
    }
    if sweep >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep + tolerance
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU) <= -sweep + tolerance
    }
}

fn point_distance(a: Point2, b: Point2) -> f64 {
    (a.u - b.u).hypot(a.v - b.v)
}

pub(super) fn closed_sketch_profiles(
    ctx: &DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> Result<Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>>, CodecError> {
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometryDefinition};

    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    let mut edges = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
    {
        if matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Ellipse { bounds: None, .. }
        ) {
            let id =
                (entity.id()).try_clone_for_decode(ctx, "f3d closed sketch profile entity id")?;
            let mut profile = Vec::new();
            ctx.push_vec(
                &mut profile,
                SketchEntityUse {
                    entity: id,
                    reversed: false,
                },
                "f3d closed sketch profile member",
            )?;
            ctx.push_vec(&mut profiles, profile, "f3d closed sketch circle profile")?;
        }
        if let Some(ends) = sketch_entity_endpoints(entity, ctx)? {
            ctx.push_vec(&mut edges, (entity, ends), "f3d closed sketch edge")?;
        }
    }
    if edges.is_empty() {
        ctx.stable_sort_by(
            &mut profiles[..],
            |a, b| a[0].entity.cmp(&b[0].entity),
            |_| 0,
            "sort f3d design geometry 4",
        )?;
        return Ok(profiles);
    }

    let mut endpoints = Vec::new();
    for (_, [start, end]) in &edges {
        ctx.push_vec(&mut endpoints, *start, "f3d closed sketch endpoint")?;
        ctx.push_vec(&mut endpoints, *end, "f3d closed sketch endpoint")?;
    }
    let mut parents = Vec::new();
    for index in 0..endpoints.len() {
        ctx.push_vec(&mut parents, index, "f3d closed sketch union parent")?;
    }
    let mut endpoint_cells = HashMap::<(i64, i64), Vec<usize>>::new();
    for (endpoint, point) in endpoints.iter().copied().enumerate() {
        let Some(u) = truncate_f64_to_i64((point.u / linear_tolerance).floor()) else {
            return Ok(Vec::new());
        };
        let Some(v) = truncate_f64_to_i64((point.v / linear_tolerance).floor()) else {
            return Ok(Vec::new());
        };
        let cell = (u, v);
        for u_offset in -1..=1 {
            for v_offset in -1..=1 {
                let Some(adjacent_u) = cell.0.checked_add(u_offset) else {
                    continue;
                };
                let Some(adjacent_v) = cell.1.checked_add(v_offset) else {
                    continue;
                };
                let adjacent = (adjacent_u, adjacent_v);
                for candidate in endpoint_cells.get(&adjacent).into_iter().flatten() {
                    if sketch_endpoints_close(point, endpoints[*candidate], linear_tolerance) {
                        union_endpoint_nodes(&mut parents, endpoint, *candidate);
                    }
                }
            }
        }
        ctx.push_hash_group(
            &mut endpoint_cells,
            cell,
            endpoint,
            "f3d sketch endpoint cell",
            "f3d sketch endpoint cell member",
        )?;
    }
    let mut edge_nodes = Vec::new();
    for edge in 0..edges.len() {
        let nodes = [
            endpoint_root(&mut parents, edge * 2),
            endpoint_root(&mut parents, edge * 2 + 1),
        ];
        ctx.push_vec(&mut edge_nodes, nodes, "f3d closed sketch edge nodes")?;
    }
    let mut adjacency = HashMap::<usize, Vec<usize>>::new();
    for (edge, [start, end]) in edge_nodes.iter().copied().enumerate() {
        ctx.push_hash_group(
            &mut adjacency,
            start,
            edge,
            "f3d sketch edge adjacency",
            "f3d sketch edge adjacency member",
        )?;
        ctx.push_hash_group(
            &mut adjacency,
            end,
            edge,
            "f3d sketch edge adjacency",
            "f3d sketch edge adjacency member",
        )?;
    }
    for incident in adjacency.values_mut() {
        ctx.stable_sort_by(
            &mut incident[..],
            |a, b| edges[*a].0.id().cmp(edges[*b].0.id()),
            |_| 0,
            "sort f3d design geometry 5",
        )?;
    }

    let mut visited = ctx.alloc_filled(edges.len(), false, "f3d edge component marks")?;
    let mut order = Vec::new();
    for edge in 0..edges.len() {
        ctx.push_vec(&mut order, edge, "f3d closed sketch edge order")?;
    }
    ctx.stable_sort_by(
        &mut order[..],
        |a, b| edges[*a].0.id().cmp(edges[*b].0.id()),
        |_| 0,
        "sort f3d design geometry 6",
    )?;
    for first_edge in order {
        if visited[first_edge] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = Vec::new();
        ctx.push_vec(&mut pending, first_edge, "f3d closed sketch pending edge")?;
        let mut component_seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !ctx.insert_hash_set(
                &mut component_seen,
                edge,
                "f3d closed sketch component seen",
            )? {
                continue;
            }
            ctx.push_vec(&mut component, edge, "f3d closed sketch component edge")?;
            for node in edge_nodes[edge] {
                for next in adjacency[&node].iter().copied() {
                    ctx.push_vec(&mut pending, next, "f3d closed sketch pending edge")?;
                }
            }
        }
        if component
            .iter()
            .flat_map(|edge| edge_nodes[*edge])
            .any(|node| adjacency[&node].len() != 2)
        {
            if component.iter().all(|edge| {
                matches!(
                    (edges[*edge].0.geometry).definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            }) {
                let branched_profiles =
                    branched_line_profiles(ctx, &component, &edges, &edge_nodes, linear_tolerance)?;
                if branched_profiles.is_empty() {
                    if let Some(profile) = tangent_nested_line_profile(
                        ctx,
                        &component,
                        &edges,
                        &edge_nodes,
                        &adjacency,
                        linear_tolerance,
                    )? {
                        ctx.push_vec(&mut profiles, profile, "f3d closed sketch tangent profile")?;
                    }
                } else {
                    for profile in branched_profiles {
                        ctx.push_vec(&mut profiles, profile, "f3d closed sketch branched profile")?;
                    }
                }
            }
            for edge in component {
                visited[edge] = true;
            }
            continue;
        }

        ctx.stable_sort_by(
            &mut component[..],
            |a, b| edges[*a].0.id().cmp(edges[*b].0.id()),
            |_| 0,
            "sort f3d design geometry 7",
        )?;
        let first_edge = component[0];
        let start_node = edge_nodes[first_edge][0];
        let mut current_node = edge_nodes[first_edge][1];
        let mut profile = Vec::new();
        let first_id = (edges[first_edge].0.id())
            .try_clone_for_decode(ctx, "f3d closed sketch component profile id")?;
        ctx.push_vec(
            &mut profile,
            SketchEntityUse {
                entity: first_id,
                reversed: false,
            },
            "f3d closed sketch component profile member",
        )?;
        visited[first_edge] = true;
        while current_node != start_node {
            let Some(next_edge) = adjacency[&current_node]
                .iter()
                .copied()
                .find(|edge| !visited[*edge])
            else {
                profile.clear();
                break;
            };
            let [stored_start, stored_end] = edge_nodes[next_edge];
            let reversed = stored_end == current_node;
            current_node = if reversed { stored_start } else { stored_end };
            visited[next_edge] = true;
            let id = (edges[next_edge].0.id())
                .try_clone_for_decode(ctx, "f3d closed sketch component profile id")?;
            ctx.push_vec(
                &mut profile,
                SketchEntityUse {
                    entity: id,
                    reversed,
                },
                "f3d closed sketch component profile member",
            )?;
        }
        if !profile.is_empty() && component.iter().all(|edge| visited[*edge]) {
            ctx.push_vec(
                &mut profiles,
                profile,
                "f3d closed sketch component profile",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut profiles[..],
        |a, b| a[0].entity.cmp(&b[0].entity),
        |_| 0,
        "sort f3d design geometry 8",
    )?;
    Ok(profiles)
}

/// Half-edge walk over one branched line component. The local `outgoing` map
/// is the only adjacency this walk needs: it is built here from `edge_nodes`,
/// so every half-edge reaches its twin by construction.
fn branched_line_profiles(
    ctx: &DecodeContext<'_>,
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    linear_tolerance: f64,
) -> Result<Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>>, CodecError> {
    use cadmpeg_ir::sketches::SketchEntityUse;

    let mut component_set = HashSet::new();
    for edge in component.iter().copied() {
        ctx.insert_hash_set(
            &mut component_set,
            edge,
            "f3d branched profile component edge",
        )?;
    }
    let mut outgoing = HashMap::<usize, Vec<usize>>::new();
    for edge in &component_set {
        ctx.push_hash_group(
            &mut outgoing,
            edge_nodes[*edge][0],
            edge * 2,
            "f3d branched profile outgoing node",
            "f3d branched profile outgoing edge",
        )?;
        ctx.push_hash_group(
            &mut outgoing,
            edge_nodes[*edge][1],
            edge * 2 + 1,
            "f3d branched profile outgoing node",
            "f3d branched profile outgoing edge",
        )?;
    }
    for half_edges in outgoing.values_mut() {
        ctx.stable_sort_by(
            &mut half_edges[..],
            |first, second| {
                let angle = |half_edge: usize| {
                    let edge = half_edge / 2;
                    let [start, end] = edges[edge].1;
                    let (from, to) = if half_edge.is_multiple_of(2) {
                        (start, end)
                    } else {
                        (end, start)
                    };
                    (to.v - from.v).atan2(to.u - from.u)
                };
                angle(*first)
                    .total_cmp(&angle(*second))
                    .then_with(|| edges[*first / 2].0.id().cmp(edges[*second / 2].0.id()))
                    .then_with(|| first.cmp(second))
            },
            |_| 0,
            "sort f3d design geometry 9",
        )?;
    }

    let mut next = HashMap::new();
    for around in outgoing.values() {
        for (&previous, &twin) in around.iter().zip(around.iter().cycle().skip(1)) {
            ctx.insert_hash_map(
                &mut next,
                twin ^ 1,
                previous,
                "f3d branched profile next half-edge",
            )
            .map(|_| ())?;
        }
    }

    let mut profiles = Vec::new();
    let mut visited = HashSet::new();
    let mut starts = Vec::new();
    for edge in &component_set {
        ctx.push_vec(
            &mut starts,
            edge * 2,
            "f3d branched profile start half-edge",
        )?;
        ctx.push_vec(
            &mut starts,
            edge * 2 + 1,
            "f3d branched profile start half-edge",
        )?;
    }
    ctx.stable_sort_by(
        &mut starts[..],
        |a, b| (edges[*a / 2].0.id(), *a % 2).cmp(&(edges[*b / 2].0.id(), *b % 2)),
        |_| 0,
        "sort f3d design geometry 10",
    )?;
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut current = start;
        let mut profile = Vec::new();
        let mut twice_area = 0.0;
        loop {
            if !ctx.insert_hash_set(
                &mut visited,
                current,
                "f3d branched profile visited half-edge",
            )? {
                if current != start {
                    profile.clear();
                }
                break;
            }
            let edge = current / 2;
            let [stored_start, stored_end] = edges[edge].1;
            let reversed = !current.is_multiple_of(2);
            let (from, to) = if reversed {
                (stored_end, stored_start)
            } else {
                (stored_start, stored_end)
            };
            twice_area += from.u * to.v - from.v * to.u;
            let id =
                (edges[edge].0.id()).try_clone_for_decode(ctx, "f3d branched profile entity id")?;
            ctx.push_vec(
                &mut profile,
                SketchEntityUse {
                    entity: id,
                    reversed,
                },
                "f3d branched profile member",
            )?;
            current = next[&current];
        }
        if !profile.is_empty() && twice_area > 2.0 * linear_tolerance * linear_tolerance {
            ctx.push_vec(&mut profiles, profile, "f3d branched profile output")?;
        }
    }

    Ok(profiles)
}

/// Resolve a tangent nested pair that has one shared corner.
///
/// The ordinary half-edge walk needs a strict angular order at every node.
/// Two profile boundaries that touch at a corner have coincident outgoing
/// rays, so that walk can consume both boundaries as one zero-area exterior
/// traversal. A neutral profile can still represent the saved face when the
/// graph proves one source-oriented outer cycle and one contained
/// oppositely-oriented inner cycle. The two cycles are then one connected
/// boundary with a tangent notch. Other tangent branch shapes remain
/// unresolved rather than choosing a profile by geometry alone.
fn tangent_nested_line_profile(
    ctx: &DecodeContext<'_>,
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    adjacency: &HashMap<usize, Vec<usize>>,
    tolerance: f64,
) -> Result<Option<Vec<cadmpeg_ir::sketches::SketchEntityUse>>, CodecError> {
    use cadmpeg_ir::sketches::SketchEntityUse;

    let mut junction = None;
    for edge in component {
        for node in edge_nodes[*edge] {
            let degree = adjacency[&node].len();
            if !matches!(degree, 2 | 4) {
                return Ok(None);
            }
            if degree > 2 {
                if junction.is_some_and(|existing| existing != node) {
                    return Ok(None);
                }
                junction = Some(node);
            }
        }
    }
    let Some(junction) = junction else {
        return Ok(None);
    };
    if adjacency[&junction].len() != 4 {
        return Ok(None);
    }
    let mut directions = [(0.0, 0.0); 4];
    for (index, edge) in adjacency[&junction].iter().enumerate() {
        let [start, end] = edges[*edge].1;
        let (from, to) = if edge_nodes[*edge][0] == junction {
            (start, end)
        } else {
            (end, start)
        };
        let (du, dv) = (to.u - from.u, to.v - from.v);
        let length = du.hypot(dv);
        if !length.is_finite() || length <= tolerance {
            return Ok(None);
        }
        directions[index] = (du / length, dv / length);
    }
    let same_ray = |left: (f64, f64), right: (f64, f64)| {
        let dot = left.0 * right.0 + left.1 * right.1;
        let cross = left.0 * right.1 - left.1 * right.0;
        dot > 1.0 - EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10
            && cross.abs() <= EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10
    };
    if directions.iter().enumerate().any(|(index, direction)| {
        directions
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter(|(_, other)| same_ray(*direction, **other))
            .count()
            != 1
    }) {
        return Ok(None);
    }

    let mut cycles = Vec::new();
    for first_edge in component
        .iter()
        .copied()
        .filter(|edge| edge_nodes[*edge][0] == junction)
    {
        let mut used = HashSet::new();
        let mut current = junction;
        let mut cycle = Vec::new();
        loop {
            let mut candidates = component
                .iter()
                .copied()
                .filter(|edge| !used.contains(edge) && edge_nodes[*edge][0] == current);
            let edge = if cycle.is_empty() {
                if edge_nodes[first_edge][0] != current {
                    return Ok(None);
                }
                first_edge
            } else {
                let Some(edge) = candidates.next() else {
                    return Ok(None);
                };
                if candidates.next().is_some() {
                    cycle.clear();
                    break;
                }
                edge
            };
            if !ctx.insert_hash_set(&mut used, edge, "f3d tangent profile used edge")? {
                cycle.clear();
                break;
            }
            ctx.push_vec(&mut cycle, edge, "f3d tangent profile cycle edge")?;
            current = edge_nodes[edge][1];
            if current == junction {
                break;
            }
        }
        if !cycle.is_empty()
            && cycle.len() >= 3
            && cycle.iter().all(|edge| component.contains(edge))
            && !cycles
                .iter()
                .any(|candidate: &Vec<usize>| candidate == &cycle)
        {
            ctx.push_vec(&mut cycles, cycle, "f3d tangent profile cycle")?;
        }
    }
    if cycles.len() != 2
        || cycles[0].iter().any(|edge| cycles[1].contains(edge))
        || cycles.iter().flatten().count() != component.len()
    {
        return Ok(None);
    }

    let cycle_points = |cycle: &[usize]| -> Result<Vec<Point2>, CodecError> {
        let mut points = Vec::new();
        for edge in cycle {
            ctx.push_vec(
                &mut points,
                edges[*edge].1[0],
                "f3d tangent profile cycle point",
            )?;
        }
        Ok(points)
    };
    let points = [cycle_points(&cycles[0])?, cycle_points(&cycles[1])?];
    let areas = [
        signed_polygon_area(&points[0]),
        signed_polygon_area(&points[1]),
    ];
    if areas
        .iter()
        .any(|area| !area.is_finite() || area.abs() <= tolerance * tolerance)
        || areas[0].signum() == areas[1].signum()
    {
        return Ok(None);
    }

    let probe = |cycle: &[usize]| {
        cycle.iter().find_map(|edge| {
            let [start, end] = edges[*edge].1;
            if edge_nodes[*edge].contains(&junction) {
                return None;
            }
            Some(Point2::new(
                (start.u + end.u) * 0.5,
                (start.v + end.v) * 0.5,
            ))
        })
    };
    let contains = |outer: usize, inner: usize| {
        probe(&cycles[inner]).is_some_and(|point| point_in_polygon(point, &points[outer]))
    };
    let outer = if areas[0].abs() > areas[1].abs() && contains(0, 1) {
        0
    } else if areas[1].abs() > areas[0].abs() && contains(1, 0) {
        1
    } else {
        return Ok(None);
    };
    let inner = 1 - outer;

    let mut profile = Vec::new();
    for (index, positive) in [(outer, true), (inner, false)] {
        let reverse = (areas[index] > 0.0) != positive;
        for position in 0..cycles[index].len() {
            let offset = if reverse {
                cycles[index].len() - 1 - position
            } else {
                position
            };
            let edge = cycles[index][offset];
            let entity =
                (edges[edge].0.id()).try_clone_for_decode(ctx, "f3d tangent profile entity id")?;
            ctx.push_vec(
                &mut profile,
                SketchEntityUse {
                    entity,
                    reversed: reverse,
                },
                "f3d tangent profile member",
            )?;
        }
    }
    let mut profile_points = Vec::new();
    for use_ in &profile {
        let Some(entity) = edges
            .iter()
            .find(|(entity, _)| entity.id() == &use_.entity)
            .map(|(entity, _)| entity)
        else {
            continue;
        };
        if let cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } =
            *entity.geometry.definition()
        {
            let point = if use_.reversed {
                end.get()
            } else {
                start.get()
            };
            ctx.push_vec(
                &mut profile_points,
                point,
                "f3d tangent profile output point",
            )?;
        }
    }
    Ok((profile_points.len() == profile.len()
        && signed_polygon_area(&profile_points) > 2.0 * tolerance * tolerance)
        .then_some(profile))
}

pub(super) fn sketch_entity_endpoints(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[Point2; 2]>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    Ok(match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some([start.get(), end.get()]),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            Point2::new(
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ),
            Point2::new(
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ),
        ]),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: Some([start_angle, end_angle]),
        } => {
            let point_at = |parameter: f64| {
                let x = radii.major().get() * parameter.cos();
                let y = radii.minor().get() * parameter.sin();
                Point2::new(
                    center.u + x * major_angle.get().cos() - y * major_angle.get().sin(),
                    center.v + x * major_angle.get().sin() + y * major_angle.get().cos(),
                )
            };
            Some([point_at(start_angle.get()), point_at(end_angle.get())])
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let lanes = PcurveEvaluatorLanes::new(
                ctx,
                curve.pole_rows(),
                "f3d nurbs evaluator poles",
                "f3d nurbs evaluator weights",
            )?;
            let start_parameter = curve.knots()[index_from_u32(curve.degree())];
            let end_parameter = curve.knots()[lanes.points().len()];
            let start = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                start_parameter,
            ))?;
            let end = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                end_parameter,
            ))?;
            start
                .zip(end)
                .map(|(start, end)| [*start.as_raw(), *end.as_raw()])
        }
        _ => None,
    })
}

fn sketch_endpoints_close(first: Point2, second: Point2, tolerance: f64) -> bool {
    (first.u - second.u).hypot(first.v - second.v) <= tolerance
}

fn endpoint_root(parents: &mut [usize], node: usize) -> usize {
    if parents[node] != node {
        parents[node] = endpoint_root(parents, parents[node]);
    }
    parents[node]
}

fn union_endpoint_nodes(parents: &mut [usize], first: usize, second: usize) {
    let first = endpoint_root(parents, first);
    let second = endpoint_root(parents, second);
    if first != second {
        parents[second] = first;
    }
}

#[cfg(test)]
mod tests;
