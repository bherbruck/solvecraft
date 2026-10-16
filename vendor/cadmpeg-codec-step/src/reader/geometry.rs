// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::{named_parameter, record_values, step_instance_id, RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{KnotVector, NurbsCurve, NurbsSurface, NurbsSurfaceAxis},
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs},
    sampled::{PolylineCurve, PolylineSamples},
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, DirectedParameterRange,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralGeometryError, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, IdentityKey, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal,
};
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::{HypotDirection2, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::SourceObjectAssociation;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::index::{CarrierIndex, CurveIndex, PointIndex, SurfaceIndex};
use super::{opaque_record_id, StageOutcome};

const EPS_GEOMETRY_READ_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_GEOMETRY_READ_GEOMETRY: f64 = 1.0e-9;
const EPS_GEOMETRY_READ_EXACT_GEOMETRY: f64 = 1.0e-12;

const RANGE_INFERENCE_WORK_UNITS: u64 = 4_096;

macro_rules! geometry_or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn attach_geometry_source(
    source: &mut Option<SourceObjectAssociation>,
    id: u64,
    name: Option<&str>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if source.is_none() {
        let name = name
            .map(|name| ctx.format_retained(format_args!("{name}"), operation))
            .transpose()?;
        *source = Some(super::step_source_association(id, name));
    }
    Ok(())
}

pub(super) struct GeometryData {
    pub(super) placements: BTreeMap<u64, (FinitePoint3, UnitVector3, UnitVector3)>,
    pub(super) transformation_operators: BTreeMap<u64, Transform>,
    pub(super) units: UnitScales,
}

pub(super) fn placement_transform(
    (origin, z_axis, x_axis): (FinitePoint3, UnitVector3, UnitVector3),
) -> Option<Transform> {
    let origin = origin.get();
    let z_axis = z_axis.as_raw();
    let x_axis = x_axis.as_raw();
    let y_axis = Vector3::new(
        z_axis.y * x_axis.z - z_axis.z * x_axis.y,
        z_axis.z * x_axis.x - z_axis.x * x_axis.z,
        z_axis.x * x_axis.y - z_axis.y * x_axis.x,
    );
    let placement_basis = [
        [x_axis.x, y_axis.x, z_axis.x],
        [x_axis.y, y_axis.y, z_axis.y],
        [x_axis.z, y_axis.z, z_axis.z],
    ];
    let mut rows = Transform::identity().affine_rows();
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = placement_basis[row][column];
        }
    }
    rows[0][3] = origin.x;
    rows[1][3] = origin.y;
    rows[2][3] = origin.z;
    Transform::affine(rows)
}

/// Infer the carrier interval trimmed by each edge's endpoint vertices.
pub(super) fn infer_edge_parameter_ranges(
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut needed = false;
    for edge in &ir.model.edges {
        ctx.charge_work(1, "step_parameter_inference_scan")?;
        if edge.param_range().is_none() && edge.curve().is_some() {
            needed = true;
            break;
        }
    }
    if !needed {
        return Ok(());
    }
    let model_index = cadmpeg_ir::index::ModelIndex::build(ir, ctx)?;
    let mut inferred = Vec::new();
    for (edge_index, edge) in ir.model.edges.iter().enumerate() {
        ctx.charge_work(1, "step_parameter_inference_scan")?;
        if edge.param_range().is_some() {
            continue;
        }
        let Some(curve) = edge.curve() else {
            continue;
        };
        let Some(start_vertex) = model_index.vertices(edge.start.as_str(), ctx)? else {
            continue;
        };
        let Some(end_vertex) = model_index.vertices(edge.end.as_str(), ctx)? else {
            continue;
        };
        let Some(start) = model_index.points(start_vertex.point.as_str(), ctx)? else {
            continue;
        };
        let Some(end) = model_index.points(end_vertex.point.as_str(), ctx)? else {
            continue;
        };
        ctx.charge_work(RANGE_INFERENCE_WORK_UNITS, "step_edge_parameter_inference")?;
        let Some(geometry) = model_index
            .curves(curve.as_str(), ctx)?
            .map(|curve| &curve.geometry)
        else {
            continue;
        };
        let Some(solved) = geometry.solved() else {
            continue;
        };
        let start_seed = curve_endpoint_seed(solved, false, 0.0);
        let Some(start_parameter) = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
            ctx,
            &model_index,
            curve,
            start.position().get(),
            start_seed,
        )?
        else {
            continue;
        };
        let end_seed = curve_endpoint_seed(solved, true, start_parameter.get());
        let Some(end_parameter) = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
            ctx,
            &model_index,
            curve,
            end.position().get(),
            end_seed,
        )?
        else {
            continue;
        };
        if let Some(range) = edge_parameter_range(solved, start_parameter, end_parameter) {
            ctx.push_vec(
                &mut inferred,
                (edge_index, range),
                "step_parameter_inference_ranges",
            )?;
        }
    }
    drop(model_index);

    for (index, range) in inferred {
        if let Some(edge) = ir.model.edges.get_mut(index) {
            let curve = edge
                .curve()
                .map(|curve| curve.try_clone_for_decode(ctx, "step_parameter_inference_edge_curve"))
                .transpose()?;
            edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(curve, Some(range))
                .map_err(CodecError::malformed)?;
        }
    }
    Ok(())
}

fn curve_endpoint_seed(geometry: &SolvedCurveGeometry, upper: bool, fallback: f64) -> f64 {
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) if !nurbs.periodic() => {
            nurbs_curve_parameter_domain(nurbs)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                .map_or(
                    fallback,
                    |[lower, upper_bound]| {
                        if upper {
                            upper_bound
                        } else {
                            lower
                        }
                    },
                )
        }
        SolvedCurveGeometry::Transformed(placed) => {
            curve_endpoint_seed(placed.basis(), upper, fallback)
        }
        _ => fallback,
    }
}

fn edge_parameter_range(
    geometry: &SolvedCurveGeometry,
    start: FiniteReal,
    end: FiniteReal,
) -> Option<[f64; 2]> {
    let periodic_domain = match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some([0.0, std::f64::consts::TAU])
        }
        SolvedCurveGeometry::Nurbs(nurbs) if nurbs.periodic() => {
            nurbs_curve_parameter_domain(nurbs)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        }
        SolvedCurveGeometry::Transformed(placed) => {
            return edge_parameter_range(placed.basis(), start, end);
        }
        _ => None,
    };
    let (start, end) = (start.get(), end.get());
    let Some([lower, upper]) = periodic_domain else {
        return (end > start).then_some([start, end]);
    };
    let period = upper - lower;
    if period <= 0.0 {
        return None;
    }
    if !period.is_finite() {
        let start = cadmpeg_ir::math::wrap_parameter(start, lower, upper)?.get();
        let end = cadmpeg_ir::math::wrap_parameter(end, lower, upper)?.get();
        let sweep = if end >= start {
            end - start
        } else {
            (upper - start) + (end - lower)
        };
        return (sweep > 0.0)
            .then_some(start + sweep)
            .filter(|end| end.is_finite())
            .map(|end| [start, end]);
    }
    let raw_span = end - start;
    let sweep = if raw_span.is_finite() {
        raw_span.rem_euclid(period)
    } else {
        (end.rem_euclid(period) - start.rem_euclid(period)).rem_euclid(period)
    };
    let tolerance = period * EPS_GEOMETRY_READ_GEOMETRY;
    if sweep <= 0.0 || sweep > period + tolerance {
        return None;
    }
    let normalized_start = cadmpeg_ir::math::wrap_parameter(start, lower, upper)?.get();
    let normalized_start = if (normalized_start - upper).abs() <= tolerance {
        lower
    } else {
        normalized_start
    };
    Some([normalized_start, normalized_start + sweep.min(period)])
}

pub(super) struct UnitScales {
    default_length: PositiveReal,
    default_angle: PositiveReal,
    length: BTreeMap<u64, PositiveReal>,
    angle: BTreeMap<u64, PositiveReal>,
}

impl UnitScales {
    pub(super) fn length(&self, ids: impl IntoIterator<Item = u64>) -> PositiveReal {
        ids.into_iter()
            .find_map(|id| self.length.get(&id).copied())
            .unwrap_or(self.default_length)
    }

    pub(super) fn angle(&self, ids: impl IntoIterator<Item = u64>) -> PositiveReal {
        ids.into_iter()
            .find_map(|id| self.angle.get(&id).copied())
            .unwrap_or(self.default_angle)
    }
}

fn resolve_source_curve_parameter_scales(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, FiniteReal>, CodecError> {
    let mut scales = BTreeMap::new();
    for &id in exchange.records().keys() {
        let mut active_storage = ctx.reserve_scoped(0, "step source curve traversal storage")?;
        if let Some(scale) = active_storage.with_storage(|| {
            source_curve_parameter_scale(id, exchange, unit_scales, &mut BTreeSet::new(), ctx)
        })? {
            ctx.insert_btree_map(&mut scales, id, scale, "step_source_curve_parameter_scales")?;
        }
    }
    Ok(scales)
}

fn source_curve_parameter_scale(
    mut id: u64,
    exchange: &Exchange,
    unit_scales: &UnitScales,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("step_source_curve_parameter_walk")?;
    loop {
        if active.contains(&id) {
            return Ok(None);
        }
        match source_curve_parameter_scale_value(id, exchange, unit_scales) {
            CurveParameterStep::Scale(scale) => return Ok(Some(scale)),
            CurveParameterStep::Parent(parent) => {
                ctx.insert_btree_set(active, id, "step_source_curve_parameter_active")?;
                id = parent;
            }
            CurveParameterStep::Absent => return Ok(None),
        }
    }
}

enum CurveParameterStep {
    Scale(FiniteReal),
    Parent(u64),
    Absent,
}

fn source_curve_parameter_scale_value(
    id: u64,
    exchange: &Exchange,
    unit_scales: &UnitScales,
) -> CurveParameterStep {
    let Some(record) = exchange.records().get(&id) else {
        return CurveParameterStep::Absent;
    };
    if record.partial("LINE").is_some() {
        let Some(magnitude) = named_parameter(record, "LINE", 2)
            .and_then(Value::reference)
            .and_then(|vector| exchange.records().get(&vector))
            .filter(|vector| vector.partial("VECTOR").is_some())
            .and_then(|vector| named_parameter(vector, "VECTOR", 2))
            .and_then(Value::number)
            .and_then(PositiveReal::new)
        else {
            return CurveParameterStep::Absent;
        };
        let scale = magnitude.get() * unit_scales.length([id]).get();
        return FiniteReal::new(scale)
            .map_or(CurveParameterStep::Absent, CurveParameterStep::Scale);
    }
    if record.partial("CIRCLE").is_some() || record.partial("ELLIPSE").is_some() {
        return CurveParameterStep::Scale(FiniteReal::from(unit_scales.angle([id])));
    }
    if record.partial("PARABOLA").is_some()
        || record.partial("HYPERBOLA").is_some()
        || record.partial("POLYLINE").is_some()
        || record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "B_SPLINE_CURVE_WITH_KNOTS"
                    | "UNIFORM_CURVE"
                    | "QUASI_UNIFORM_CURVE"
                    | "BEZIER_CURVE"
            )
        })
    {
        return CurveParameterStep::Scale(FiniteReal::ONE);
    }
    let parent = ["CURVE_REPLICA", "TRIMMED_CURVE", "OFFSET_CURVE_3D"]
        .into_iter()
        .find_map(|name| {
            named_parameter(record, name, 1)
                .and_then(Value::reference)
                .and_then(|parent| curve_carrier_record(parent, exchange))
        })
        .or_else(|| {
            record
                .partials
                .iter()
                .any(|partial| {
                    matches!(
                        partial.name.as_str(),
                        "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                    )
                })
                .then(|| surface_curve_basis(record))
                .flatten()
        });
    match parent {
        Some(parent) => CurveParameterStep::Parent(parent),
        None => CurveParameterStep::Absent,
    }
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<(StageOutcome<GeometryData>, CarrierIndex), CodecError> {
    // This scope appends carriers and preserves their identity and construction links.
    let mut procedural_admission =
        cadmpeg_ir::document::procedural::ProceduralAdmission::new(ctx, &ir.model)?;

    let mut losses = Vec::new();
    let scale = match length_scale(exchange, ctx)? {
        Some(scale) => scale,
        None => {
            ctx.push_vec(&mut losses, StepLossCode::DocumentLengthUnitUnresolved.note(
                    "the document length unit did not resolve; coordinates are unscaled and reported as millimetres",
                ), "step_geometry_losses")?;
            PositiveReal::ONE
        }
    };
    let angle_scale = match plane_angle_scale(exchange, ctx)? {
        Some(scale) => scale,
        None => {
            ctx.push_vec(&mut losses, StepLossCode::DocumentAngleUnitUnresolved.note(
                    "the document plane-angle unit did not resolve; angles are unscaled and reported as radians",
                ), "step_geometry_losses")?;
            PositiveReal::ONE
        }
    };
    let unit_scales = resolve_unit_scales(exchange, scale, angle_scale, &mut losses, ctx)?;
    let angle_scale = angle_scale.get();
    let source_curve_parameter_scales =
        resolve_source_curve_parameter_scales(exchange, &unit_scales, ctx)?;
    let mut typed = HashSet::new();
    let mut points = BTreeMap::new();
    let mut points2 = BTreeMap::new();
    let mut apll_point_names = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut directions2 = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut vectors2 = BTreeMap::new();
    let mut placements = BTreeMap::new();
    let mut placements2 = BTreeMap::new();
    match linear_uncertainty(exchange, ctx)? {
        LinearUncertainty::Value(uncertainty) => ir.tolerances.linear = uncertainty,
        LinearUncertainty::Empty { unresolved } => {
            if unresolved > 0 {
                ctx.push_vec(&mut losses, StepLossCode::UncertaintyLengthUnresolved.note(format!(
                    "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT has no resolvable length measure and {unresolved} unresolved measure(s); the linear tolerance was not transferred"
                )), "step_geometry_losses")?;
            }
        }
        LinearUncertainty::Ambiguous {
            first,
            second,
            rest,
            unresolved,
        } => {
            let default_linear = ir.tolerances.linear.get();
            let listed = ctx.join_display_retained(
                [first, second]
                    .iter()
                    .chain(&rest)
                    .map(|value| format!("{:?}", value.get())),
                ", ",
                "step_uncertainty_values_text",
            )?;
            let message = ctx.format_retained(format_args!(
                "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT records give {} different linear uncertainty values in millimetres ({listed}) and {unresolved} unresolved measure(s); the linear tolerance keeps the default {default_linear:?}",
                2 + rest.len()
            ), "step_uncertainty_note_text")?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::UncertaintyLengthAmbiguous.note(message),
                "step_geometry_losses",
            )?;
        }
    }

    for (id, record) in exchange.entities_any(&[
        "APLL_POINT",
        "APLL_POINT_WITH_SURFACE",
        "CARTESIAN_POINT",
        "DIRECTION",
    ]) {
        match entity_type(
            record,
            &[
                "APLL_POINT",
                "APLL_POINT_WITH_SURFACE",
                "CARTESIAN_POINT",
                "DIRECTION",
            ],
        ) {
            Some(point_type @ ("APLL_POINT" | "APLL_POINT_WITH_SURFACE")) => {
                let record_scale = unit_scales.length([id]).get();
                if let Some(position) = apll_point_coordinates(record, point_type, record_scale) {
                    ctx.insert_btree_map(&mut points, id, position, "step_geometry_points")?;
                    let source_name = representation_item_name(record)
                        .map(|value| {
                            super::decode_text_charged(
                                exchange,
                                value,
                                &mut losses,
                                id,
                                "APLL point name",
                                StepLossCode::MetadataStringInvalid,
                                ctx,
                            )
                        })
                        .transpose()?
                        .flatten()
                        .filter(|name| !name.is_empty());
                    ctx.insert_btree_map(
                        &mut apll_point_names,
                        id,
                        source_name,
                        "step_geometry_apll_point_names",
                    )?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning
                            .note(format!("{point_type} #{id} has invalid coordinates")),
                        "step_geometry_losses",
                    )?;
                }
            }
            Some("CARTESIAN_POINT") => {
                let record_scale = unit_scales.length([id]).get();
                if let Some(position) =
                    named_coordinates(record, "CARTESIAN_POINT", 1, record_scale)
                {
                    ctx.insert_btree_map(&mut points, id, position, "step_geometry_points")?;
                    ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else if let Some(position) = named_coordinates2(record, "CARTESIAN_POINT", 1) {
                    ctx.insert_btree_map(&mut points2, id, position, "step_geometry_points2")?;
                    ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning
                            .note(format!("CARTESIAN_POINT #{id} has invalid coordinates")),
                        "step_geometry_losses",
                    )?;
                }
            }
            Some("DIRECTION") => {
                if let Some(direction) =
                    vector3(named_parameter(record, "DIRECTION", 1), 1.0).and_then(normalize)
                {
                    ctx.insert_btree_map(
                        &mut directions,
                        id,
                        direction,
                        "step_geometry_directions",
                    )?;
                    ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else if let Some(direction) = direction2(named_parameter(record, "DIRECTION", 1))
                {
                    ctx.insert_btree_map(
                        &mut directions2,
                        id,
                        direction,
                        "step_geometry_directions2",
                    )?;
                    ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning
                            .note(format!("DIRECTION #{id} is invalid or zero")),
                        "step_geometry_losses",
                    )?;
                }
            }
            _ => {}
        }
    }
    decode_tessellated_curve_sets(exchange, &unit_scales, ir, &mut typed, &mut losses, ctx)?;
    let mut point_carriers = BTreeSet::new();
    for record in exchange.records().values() {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "VERTEX_POINT")
        {
            if let Some(id) = vertex_point_reference(record) {
                ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| super::representation::is_representation_name(&partial.name))
        {
            if let Some(items) = representation_items(record) {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
            )
        }) {
            if let Some(items) = first_named_list(record, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])
            {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "POLY_LOOP")
        {
            if let Some(items) = first_named_list(record, &["POLY_LOOP"]) {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if is_apll_leader_line(record) {
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
            {
                for id in super::reference::references(parameter, ctx) {
                    let id = id?;
                    if points.contains_key(&id) {
                        ctx.insert_btree_set(
                            &mut point_carriers,
                            id,
                            "step_geometry_point_carriers",
                        )?;
                    }
                }
            }
        }
        if let Some(item) = record
            .partials
            .iter()
            .find(|partial| {
                matches!(
                    partial.name.as_str(),
                    "GEOMETRIC_ITEM_SPECIFIC_USAGE" | "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
                )
            })
            .and_then(|partial| partial.parameters.get(4))
            .and_then(Value::reference)
        {
            if points.contains_key(&item) {
                ctx.insert_btree_set(&mut point_carriers, item, "step_geometry_point_carriers")?;
            }
        }
        if let Some(id) = super::presentation::styled_item_target(record) {
            if points.contains_key(&id) {
                ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
            }
        }
    }
    for id in point_carriers {
        let Some(position) = points.get(&id).copied() else {
            continue;
        };
        let source_name = apll_point_names.remove(&id);
        ctx.push_vec(
            &mut ir.model.points,
            Point::new(
                PointId::from(ids::data(kind!("point"), id)),
                position,
                source_name.map(|name| super::step_source_association(id, name)),
            ),
            "step_geometry_ir_points",
        )?;
    }
    for (id, record) in exchange.entities("VECTOR") {
        if record.partial("VECTOR").is_some() {
            let record_scale = unit_scales.length([id]).get();
            let value = named_parameter(record, "VECTOR", 1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(named_parameter(record, "VECTOR", 2).and_then(Value::number))
                .map(|(direction, magnitude)| direction.as_raw().scale(magnitude * record_scale));
            let value2 = named_parameter(record, "VECTOR", 1)
                .and_then(Value::reference)
                .and_then(|direction| directions2.get(&direction).copied())
                .zip(named_parameter(record, "VECTOR", 2).and_then(Value::number))
                .map(|(direction, magnitude)| {
                    let [u, v] = direction.get();
                    Point2::new(u * magnitude, v * magnitude)
                });
            if let Some(value) = value {
                ctx.insert_btree_map(&mut vectors, id, value, "step_geometry_vectors")?;
                ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else if let Some(value) = value2 {
                ctx.insert_btree_map(&mut vectors2, id, value, "step_geometry_vectors2")?;
                ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "VECTOR #{id} has an invalid direction or magnitude"
                    )),
                    "step_geometry_losses",
                )?;
            }
        }
    }
    for (id, record) in exchange.entities_any(&["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"]) {
        let Some(placement_type) = entity_type(record, &["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"])
        else {
            continue;
        };
        {
            let mut inferred_reference = false;
            let placement = named_parameter(record, placement_type, 1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis =
                        optional_direction(named_parameter(record, placement_type, 2), &directions)
                            .unwrap_or(UnitVector3::Z_AXIS);
                    let reference = match optional_direction(
                        named_parameter(record, placement_type, 3),
                        &directions,
                    ) {
                        Some(reference) => {
                            if let Some(reference) = orthogonal_reference(axis, reference) {
                                reference
                            } else {
                                inferred_reference = true;
                                first_projected_axis(axis).unwrap_or(UnitVector3::X_AXIS)
                            }
                        }
                        None => first_projected_axis(axis).unwrap_or(UnitVector3::X_AXIS),
                    };
                    (origin, axis, reference)
                });
            if inferred_reference {
                ctx.push_vec(&mut losses, StepLossCode::PlacementReferenceInferred.note(format!(
                        "AXIS2_PLACEMENT_3D #{id} has a reference direction parallel to its axis; inferred an orthogonal reference"
                    )), "step_geometry_losses")?;
            }
            if let Some(placement) = placement {
                ctx.insert_btree_map(&mut placements, id, placement, "step_geometry_placements")?;
                ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning
                        .note(format!("AXIS2_PLACEMENT_3D #{id} has an invalid location")),
                    "step_geometry_losses",
                )?;
            }
        }
    }
    let mut transformation_operators = BTreeMap::new();
    for (id, record) in exchange.entities("CARTESIAN_TRANSFORMATION_OPERATOR_3D") {
        if let Some(transform) = cartesian_transformation_operator(record, &points, &directions) {
            ctx.insert_btree_map(
                &mut transformation_operators,
                id,
                transform,
                "step_geometry_transformation_operators",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CARTESIAN_TRANSFORMATION_OPERATOR_3D #{id} has invalid axes, origin, or scale"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    let mut transformation_operators2 = BTreeMap::new();
    for (id, record) in exchange.entities("CARTESIAN_TRANSFORMATION_OPERATOR_2D") {
        if let Some(transform) =
            cartesian_transformation_operator_2d(record, &points2, &directions2)
        {
            ctx.insert_btree_map(
                &mut transformation_operators2,
                id,
                transform,
                "step_geometry_transformation_operators2",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CARTESIAN_TRANSFORMATION_OPERATOR_2D #{id} has invalid axes, origin, or scale"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities("AXIS2_PLACEMENT_2D") {
        if record.partial("AXIS2_PLACEMENT_2D").is_none() {
            continue;
        }
        let placement = named_parameter(record, "AXIS2_PLACEMENT_2D", 1)
            .and_then(Value::reference)
            .and_then(|point| points2.get(&point).copied())
            .and_then(|origin| {
                let x_axis = match named_parameter(record, "AXIS2_PLACEMENT_2D", 2) {
                    Some(Value::Reference(direction)) => directions2.get(direction).copied()?,
                    Some(Value::Omitted) | None => HypotDirection2::X_AXIS,
                    _ => return None,
                };
                Some((origin, x_axis, x_axis.quarter_turn()))
            });
        if let Some(placement) = placement {
            ctx.insert_btree_map(&mut placements2, id, placement, "step_geometry_placements2")?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("AXIS2_PLACEMENT_2D #{id} has an invalid location")),
                "step_geometry_losses",
            )?;
        }
    }
    let mut pcurve_geometries = BTreeMap::<u64, (PcurveGeometry, BTreeSet<u64>)>::new();
    let mut pcurve_geometry_records = BTreeSet::new();
    for (_, record) in exchange.entities("PCURVE") {
        if record.partial("PCURVE").is_none() {
            continue;
        }
        let representation_id = named_parameter(record, "PCURVE", 2).and_then(Value::reference);
        let Some(items) = representation_id
            .and_then(|representation| exchange.records().get(&representation))
            .and_then(representation_items)
        else {
            continue;
        };
        let pcurve_angle_scale = representation_id.map_or(angle_scale, |representation| {
            unit_scales.angle([representation]).get()
        });
        let mut decoded = None;
        let mut decoded_count = 0;
        for curve in items {
            if let Some(geometry) = decode_pcurve_geometry(
                curve,
                exchange,
                PcurveSources {
                    points: &points2,
                    vectors: &vectors2,
                    placements: &placements2,
                    transformations: &transformation_operators2,
                    angle_scale: pcurve_angle_scale,
                },
                &mut losses,
                &mut BTreeSet::new(),
                0,
                ctx,
            )? {
                decoded_count += 1;
                decoded = Some((curve, geometry));
            }
        }
        if decoded_count == 1 {
            if let Some((curve, (geometry, records))) = decoded {
                for &record in &records {
                    ctx.insert_btree_set(
                        &mut pcurve_geometry_records,
                        record,
                        "step_pcurve_geometry_records",
                    )?;
                }
                ctx.insert_btree_map(
                    &mut pcurve_geometries,
                    curve,
                    (geometry, records),
                    "step_pcurve_geometries",
                )?;
            }
        }
    }
    let mut curve_parameter_offsets = BTreeMap::<u64, f64>::new();
    for (id, record) in exchange.entities_any(LeafCurveEntity::NAMES) {
        let Some(curve_kind) = LeafCurveEntity::of(record) else {
            continue;
        };
        if pcurve_geometry_records.contains(&id) {
            continue;
        }
        if curve_kind == LeafCurveEntity::BSplineWithKnots && record.simple_name().is_none() {
            continue;
        }
        let record_scale = unit_scales.length([id]).get();
        let parameter_offset = if curve_kind == LeafCurveEntity::Ellipse {
            let first_radius = named_parameter(record, "ELLIPSE", 2).and_then(Value::number);
            let second_radius = named_parameter(record, "ELLIPSE", 3).and_then(Value::number);
            first_radius
                .zip(second_radius)
                .filter(|(first, second)| first.is_finite() && second.is_finite())
                .and_then(|(first, second)| {
                    (first < second).then_some(-std::f64::consts::FRAC_PI_2)
                })
        } else {
            None
        };
        let geometry = match curve_kind {
            LeafCurveEntity::Line => named_parameter(record, "LINE", 1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    named_parameter(record, "LINE", 2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction),
                    ))
                }),
            LeafCurveEntity::Circle => named_parameter(record, "CIRCLE", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "CIRCLE", 2).and_then(Value::number))
                .and_then(|((center, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = PositiveLength::new(radius * record_scale)?;
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                        cadmpeg_ir::geometry::analytic::CircleCurve::new(center, frame, radius),
                    )))
                }),
            LeafCurveEntity::Ellipse => named_parameter(record, "ELLIPSE", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "ELLIPSE", 2).and_then(Value::number))
                .zip(named_parameter(record, "ELLIPSE", 3).and_then(Value::number))
                .and_then(
                    |(((center, axis, reference_direction), first_radius), second_radius)| {
                        let first_radius = first_radius * record_scale;
                        let second_radius = second_radius * record_scale;
                        let (major_direction, major_radius, minor_radius) =
                            if first_radius >= second_radius {
                                (*reference_direction.as_raw(), first_radius, second_radius)
                            } else {
                                // STEP ELLIPSE stores two ordered semiaxes;
                                // neither position is required to be the
                                // longer one. The IR ellipse is canonicalized
                                // around its semi-major direction.
                                (
                                    axis.as_raw().cross(*reference_direction.as_raw()),
                                    second_radius,
                                    first_radius,
                                )
                            };
                        let major_direction = UnitVector3::new(major_direction)?;
                        let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                        let major_radius = PositiveLength::new(major_radius)?;
                        let minor_radius = PositiveLength::new(minor_radius)?;
                        cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                            center,
                            frame,
                            major_radius,
                            minor_radius,
                        )
                        .ok()
                        .map(SolvedCurveGeometry::Ellipse)
                        .map(CurveGeometry::Solved)
                    },
                ),
            LeafCurveEntity::Parabola => named_parameter(record, "PARABOLA", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "PARABOLA", 2).and_then(Value::number))
                .and_then(|((vertex, axis, major_direction), focal_distance)| {
                    let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                    let focal_distance = PositiveLength::new(focal_distance * record_scale)?;
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                        cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                            vertex,
                            frame,
                            focal_distance,
                        ),
                    )))
                }),
            LeafCurveEntity::Hyperbola => named_parameter(record, "HYPERBOLA", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "HYPERBOLA", 2).and_then(Value::number))
                .zip(named_parameter(record, "HYPERBOLA", 3).and_then(Value::number))
                .and_then(
                    |(((center, axis, major_direction), major_radius), minor_radius)| {
                        let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                        let major_radius = PositiveLength::new(major_radius * record_scale)?;
                        let minor_radius = PositiveLength::new(minor_radius * record_scale)?;
                        Some(CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                            cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                                center,
                                frame,
                                major_radius,
                                minor_radius,
                            ),
                        )))
                    },
                ),
            LeafCurveEntity::Polyline => polyline(id, record, &points, &mut losses, ctx)?
                .map(SolvedCurveGeometry::Nurbs)
                .map(CurveGeometry::Solved),
            LeafCurveEntity::BSplineWithKnots
            | LeafCurveEntity::UniformCurve
            | LeafCurveEntity::QuasiUniformCurve
            | LeafCurveEntity::BezierCurve => nurbs_curve(id, record, &points, &mut losses, ctx)?
                .map(SolvedCurveGeometry::Nurbs)
                .map(CurveGeometry::Solved),
        };
        if let Some(geometry) = geometry {
            if let Some(offset) = parameter_offset {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry,
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("{} #{id} has invalid geometry", curve_kind.name())),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_CURVE_WITH_KNOTS") {
        if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_CURVE_WITH_KNOTS")
            || pcurve_geometry_records.contains(&id)
        {
            continue;
        }
        if let Some(nurbs) = nurbs_curve(id, record, &points, &mut losses, ctx)? {
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_CURVE_WITH_KNOTS #{id} has invalid geometry"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    // STEP geometry is a graph, not an ordered stream. Resolve all deferred
    // curve constructors to a fixpoint so nested or forward references do not
    // disappear merely because their source record has a larger instance id.
    let mut carrier_index = CarrierIndex::from_ir(ir, ctx)?;
    let mut deferred_ids = Vec::new();
    for (id, _) in exchange
        .entities_any(&[
            "CURVE_REPLICA",
            "TRIMMED_CURVE",
            "COMPOSITE_CURVE",
            "BOUNDARY_CURVE",
            "OUTER_BOUNDARY_CURVE",
            "OFFSET_CURVE_3D",
        ])
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        ctx.push_vec(&mut deferred_ids, id, "step_deferred_curve_ids")?;
    }
    let mut deferred_queue = VecDeque::from(deferred_ids);
    let mut waiting_on = HashMap::<u64, Vec<u64>>::new();
    while let Some(id) = deferred_queue.pop_front() {
        if carrier_index.curves.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if let Some(parent_reference_step) = record
            .partial("CURVE_REPLICA")
            .and_then(|_| named_parameter(record, "CURVE_REPLICA", 1))
            .and_then(Value::reference)
        {
            let Some(parent_step) = curve_carrier_record(parent_reference_step, exchange) else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(record, "CURVE_REPLICA", 2).and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.curves.get(&parent_step).copied() else {
                defer_geometry_dependency(
                    &mut waiting_on,
                    parent_step,
                    id,
                    ctx,
                    "step_deferred_curve_groups",
                    "step_deferred_curve_members",
                )?;
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .curves
                .get(parent_index.0)
                .and_then(|curve| curve.geometry.solved())
            else {
                continue;
            };
            let basis = basis.try_clone_for_decode(ctx, "step_curve_replica_basis")?;
            let Ok(placed) = cadmpeg_ir::geometry::PlacedCurve::try_new(Box::new(basis), transform)
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "CURVE_REPLICA #{id} nests past the admitted inline basis depth"
                    )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            let geometry = SolvedCurveGeometry::Transformed(placed);
            let curve_index = CurveIndex(ir.model.curves.len());
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let procedural = ProceduralCurve::new(
                ProceduralCurveId::from(ids::construction(kind!("curve_replica"), id)),
                ProceduralCurveDefinition::Replica {
                    source: CurveId::from(ids::data(kind!("curve"), parent_step)),
                    transform,
                },
            );
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(geometry),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            let _attached = procedural_admission.add_curve(&mut ir.model, &curve, procedural)?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            if let Some(offset) = curve_parameter_offsets.get(&parent_step).copied() {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            ctx.insert_hash_set(&mut typed, operator_step, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        if let Some(parameters) = entity_parameters(record, "TRIMMED_CURVE") {
            let Some((basis_reference_step, sense, master_representation)) =
                trimmed_curve_attributes(parameters)
            else {
                continue;
            };
            let Some(basis_step) = curve_carrier_record(basis_reference_step, exchange) else {
                continue;
            };
            if !carrier_index.curves.contains_key(&basis_step) {
                defer_geometry_dependency(
                    &mut waiting_on,
                    basis_step,
                    id,
                    ctx,
                    "step_deferred_curve_groups",
                    "step_deferred_curve_members",
                )?;
                continue;
            }
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let basis = CurveId::from(ids::data(kind!("curve"), basis_step));
            let Some(geometry) = carrier_index
                .curves
                .get(&basis_step)
                .and_then(|index| ir.model.curves.get(index.0))
                .map(|candidate| &candidate.geometry)
            else {
                continue;
            };
            let Some(solved_geometry) = geometry.solved() else {
                continue;
            };
            let record_scale = unit_scales.length([id]);
            let record_angle_scale = unit_scales.angle([id]).get();
            let parameter_offset = curve_parameter_offsets
                .get(&basis_step)
                .copied()
                .unwrap_or(0.0);
            let linear_parameter_scale = line_parameter_scale(
                exchange,
                basis_reference_step,
                record_scale,
                &mut losses,
                ctx,
            )?;
            let (start, end) = {
                let mut trim_context = TrimParameterContext {
                    points: &points,
                    geometry,
                    angle_scale: record_angle_scale,
                    linear_parameter_scale: linear_parameter_scale.get(),
                    parameter_offset,
                    tolerance: ir.tolerances.linear.get(),
                    master_representation,
                    record_id: id,
                    losses: &mut losses,
                    ctx,
                };
                (
                    match parameters.get(2) {
                        Some(value) => trim_parameter(value, &mut trim_context)?,
                        None => None,
                    },
                    match parameters.get(3) {
                        Some(value) => trim_parameter(value, &mut trim_context)?,
                        None => None,
                    },
                )
            };
            let Some((start, end)) = start.zip(end) else {
                continue;
            };
            let parameter_range = trimmed_curve_parameter_range(geometry, start, end, sense);
            let procedural =
                match cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                    basis,
                    parameter_range,
                    sense,
                    None,
                )
                .and_then(|admitted_payload| {
                    let mut definition = ProceduralCurveDefinition::Subset(admitted_payload);
                    definition.set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::geometry::FitTolerance::ZERO,
                    ))?;
                    Ok(ProceduralCurve::new(
                        ProceduralCurveId::from(ids::construction(kind!("trimmed_curve"), id)),
                        definition,
                    ))
                }) {
                    Ok(procedural) => procedural,
                    Err(error) => {
                        ctx.push_vec(
                            &mut losses,
                            StepLossCode::DecodeWarning
                                .note(format!("TRIMMED_CURVE #{id}: {error}")),
                            "step_geometry_losses",
                        )?;
                        continue;
                    }
                };
            let curve_index = CurveIndex(ir.model.curves.len());
            let copied_geometry =
                solved_geometry.try_clone_for_decode(ctx, "step_trim_curve_carrier")?;
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;

            let _attached = procedural_admission.add_curve(&mut ir.model, &curve, procedural)?;

            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            if parameter_offset != 0.0 {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    parameter_offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        if composite_curve_parameters(record).is_some() {
            let mut missing = false;
            for dependency in composite_curve_dependencies(record, exchange) {
                if !carrier_index.curves.contains_key(&dependency) {
                    defer_geometry_dependency(
                        &mut waiting_on,
                        dependency,
                        id,
                        ctx,
                        "step_deferred_curve_groups",
                        "step_deferred_curve_members",
                    )?;
                    missing = true;
                }
            }
            if missing {
                continue;
            }
            let Some((segments, self_intersect)) =
                composite_curve(record, exchange, &carrier_index, ctx)?
            else {
                continue;
            };
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            for &(segment, _) in &segments {
                ctx.insert_hash_set(&mut typed, segment, "step_geometry_typed_ids")?;
            }
            let mut model_segments = Vec::new();
            for (_, segment) in segments {
                ctx.push_vec(
                    &mut model_segments,
                    segment,
                    "step_composite_curve_model_segments",
                )?;
            }
            let segments =
                match cadmpeg_ir::geometry::CompositeCurveSegments::try_from(model_segments) {
                    Ok(segments) => segments,
                    Err(error) => {
                        ctx.push_vec(
                            &mut losses,
                            StepLossCode::DecodeWarning
                                .note(format!("COMPOSITE_CURVE #{id}: {error}")),
                            "step_geometry_losses",
                        )?;
                        continue;
                    }
                };
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                        segments,
                        self_intersect,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        let Some(parameters) = entity_parameters(record, "OFFSET_CURVE_3D") else {
            continue;
        };
        let source_reference_step = parameters.get(1).and_then(Value::reference);
        let source_step =
            source_reference_step.and_then(|source| curve_carrier_record(source, exchange));
        let source = source_step.map(|source| CurveId::from(ids::data(kind!("curve"), source)));
        let distance = parameters.get(2).and_then(Value::number);
        let self_intersect = parameters
            .get(3)
            .and_then(|value| logical_value(value).ok());
        let reference_direction = parameters
            .get(4)
            .and_then(Value::reference)
            .and_then(|direction| directions.get(&direction).copied());
        let Some((source, distance, self_intersect, reference_direction)) = source
            .zip(distance)
            .zip(self_intersect)
            .zip(reference_direction)
            .map(|(((source, distance), self_intersect), direction)| {
                (source, distance, self_intersect, direction)
            })
        else {
            continue;
        };
        let Some(source_step) = source_step else {
            continue;
        };
        if !carrier_index.curves.contains_key(&source_step) {
            defer_geometry_dependency(
                &mut waiting_on,
                source_step,
                id,
                ctx,
                "step_deferred_curve_groups",
                "step_deferred_curve_members",
            )?;
            continue;
        }
        let Some(geometry) = carrier_index
            .curves
            .get(&source_step)
            .and_then(|index| ir.model.curves.get(index.0))
            .and_then(|candidate| candidate.geometry.solved())
        else {
            continue;
        };
        let curve = CurveId::from(ids::data(kind!("curve"), id));
        let curve_index = CurveIndex(ir.model.curves.len());
        let copied_geometry = geometry.try_clone_for_decode(ctx, "step_offset_curve_carrier")?;
        let procedural =
            match cadmpeg_ir::geometry::curve_payloads::SpatialOffsetCurveConstruction::try_from_parts(
                source,
                distance * unit_scales.length([id]).get(),
                reference_direction,
                self_intersect,
            )
            .map(|admitted_payload| {
                ProceduralCurve::new(
                    ProceduralCurveId::from(ids::construction(kind!("offset_curve"), id)),
                    ProceduralCurveDefinition::SpatialOffset(admitted_payload),
                )
            }) {
                Ok(procedural) => procedural,
                Err(error) => {
                    ctx.push_vec(&mut losses, StepLossCode::DecodeWarning
                            .note(format!("curve construction #{id}: {error}")), "step_geometry_losses")?;
                    continue;
                }
            };
        ctx.push_vec(
            &mut ir.model.curves,
            Curve {
                parameter_range: None,
                id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                geometry: CurveGeometry::Solved(copied_geometry),
                source_object: None,
            },
            "step_geometry_ir_curves",
        )?;
        let _attached = procedural_admission.add_curve(&mut ir.model, &curve, procedural)?;
        ctx.insert_hash_map(
            &mut carrier_index.curves,
            id,
            curve_index,
            "step_geometry_curve_index",
        )?;
        if let Some(offset) = curve_parameter_offsets.get(&source_step).copied() {
            ctx.insert_btree_map(
                &mut curve_parameter_offsets,
                id,
                offset,
                "step_geometry_curve_parameter_offsets",
            )?;
        }
        ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        wake_deferred_dependents(
            id,
            &mut waiting_on,
            &mut deferred_queue,
            ctx,
            "step_deferred_curve_queue",
        )?;
    }
    for (id, _) in exchange
        .entities("CURVE_REPLICA")
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CURVE_REPLICA #{id} has invalid or unresolved parent/operator"
                )),
                "step_geometry_losses",
            )?;
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for (id, _) in exchange
        .entities("TRIMMED_CURVE")
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TRIMMED_CURVE #{id} has invalid or unresolved basis/trim selectors"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in
        exchange.entities_any(&["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"])
    {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "{} #{id} has invalid, cyclic, or unresolved segments",
                    record.simple_name().unwrap_or("COMPOSITE_CURVE")
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities("OFFSET_CURVE_3D") {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "OFFSET_CURVE_3D #{id} has invalid or unresolved basis parameters"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange
        .entities_any(&[
            "TRIMMED_CURVE",
            "COMPOSITE_CURVE",
            "BOUNDARY_CURVE",
            "OUTER_BOUNDARY_CURVE",
            "OFFSET_CURVE_3D",
        ])
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if !carrier_index.curves.contains_key(&id) {
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained unresolved deferred curve #{id} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for (id, record) in
        exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])
    {
        let Some(basis) = surface_curve_basis(record) else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "{} #{id} has no decoded 3D curve",
                    record.simple_name().unwrap_or("SURFACE_CURVE")
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        if carrier_index.curves.contains_key(&basis) {
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "{} #{id} has no decoded 3D curve",
                    record.simple_name().unwrap_or("SURFACE_CURVE")
                )),
                "step_geometry_losses",
            )?;
        }
    }

    for (id, record) in
        exchange.entities_any(&["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"])
    {
        let (surface_type, definition) = match entity_type(
            record,
            &["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"],
        ) {
            Some(surface_type @ "SURFACE_OF_LINEAR_EXTRUSION") => (surface_type,
                named_parameter(record, "SURFACE_OF_LINEAR_EXTRUSION", 1)
                    .and_then(Value::reference)
                    .filter(|curve| carrier_index.curves.contains_key(curve))
                    .map(|curve| CurveId::from(ids::data(kind!("curve"), curve)))
                    .zip(
                        named_parameter(record, "SURFACE_OF_LINEAR_EXTRUSION", 2)
                            .and_then(Value::reference)
                            .and_then(|vector| vectors.get(&vector).copied()),
                    )
                    .map(|(directrix, direction)| {
                        cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                            directrix, direction,
                        )
                        .map(ProceduralSurfaceDefinition::LinearSweep)
                    })
            ),
            Some(surface_type @ "SURFACE_OF_REVOLUTION") => (surface_type, named_parameter(record, "SURFACE_OF_REVOLUTION", 1)
                .and_then(Value::reference)
                .filter(|curve| carrier_index.curves.contains_key(curve))
                .map(|curve| CurveId::from(ids::data(kind!("curve"), curve)))
                .zip(
                    named_parameter(record, "SURFACE_OF_REVOLUTION", 2)
                        .and_then(Value::reference)
                        .and_then(|placement| placements.get(&placement).copied()),
                )
                .map(|(directrix, (axis_origin, axis_direction, _))| {
                    let construction =
                        cadmpeg_ir::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::from_parts(
                            directrix,
                            axis_origin,
                            axis_direction,
                        );
                    Ok(ProceduralSurfaceDefinition::AxisRevolution(construction))
                })),
            _ => continue,
        };
        let Some(definition) = definition else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "{surface_type} #{id} has an unresolved directrix, vector, or axis"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let definition = match definition {
            Ok(definition) => definition,
            Err(error) => {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!("procedural surface #{id}: {error}")),
                    "step_geometry_losses",
                )?;
                continue;
            }
        };
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        ctx.push_vec(
            &mut ir.model.surfaces,
            Surface {
                id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            },
            "step_geometry_ir_surfaces",
        )?;
        let _attached = procedural_admission.add_surface(
            &mut ir.model,
            &surface,
            ProceduralSurface::new(
                ProceduralSurfaceId::from(ids::construction(kind!("swept_surface"), id)),
                definition,
                None,
            ),
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
    }

    for (id, record) in exchange.entities_any(LeafSurfaceEntity::NAMES) {
        let Some(surface_kind) = LeafSurfaceEntity::of(record) else {
            continue;
        };
        let surface_type = surface_kind.name();
        if surface_kind == LeafSurfaceEntity::BSplineWithKnots && record.simple_name().is_none() {
            continue;
        }
        let record_scale = unit_scales.length([id]).get();
        let record_angle_scale = unit_scales.angle([id]).get();
        let placement = named_parameter(record, surface_type, 1)
            .and_then(Value::reference)
            .and_then(|placement| placements.get(&placement).copied());
        let geometry = match surface_kind {
            LeafSurfaceEntity::Plane => placement.and_then(|(origin, normal, u_axis)| {
                let frame = OrthonormalFrame3::from_units(normal, u_axis)?;
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame),
                )))
            }),
            LeafSurfaceEntity::Cylindrical => placement
                .zip(named_parameter(record, "CYLINDRICAL_SURFACE", 2).and_then(Value::number))
                .and_then(|((origin, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = PositiveLength::new(radius * record_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                        cadmpeg_ir::geometry::analytic::CylinderSurface::new(origin, frame, radius),
                    )))
                }),
            // ISO 10303-42 `conical_surface` holds `semi_angle` in `(0, pi/2)`.
            // Every finite value comes through, because the IR cone uses the
            // chart STEP states and reproduces the surface the record names at
            // any slope. Each arm of this match refuses what its IR carrier
            // refuses and nothing more: the cone radius stops at
            // `NonNegativeLength`, which is the entity's own `radius >= 0`, and
            // the toroidal arm below serves `TOROIDAL_SURFACE` and
            // `DEGENERATE_TOROIDAL_SURFACE` without separating their radius
            // rules. A refused carrier withholds the one surface with a
            // `DecodeWarning` note and leaves the rest of the decode standing.
            LeafSurfaceEntity::Conical => placement
                .zip(named_parameter(record, "CONICAL_SURFACE", 2).and_then(Value::number))
                .zip(named_parameter(record, "CONICAL_SURFACE", 3).and_then(Value::number))
                .and_then(|(((origin, axis, ref_direction), radius), half_angle)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = NonNegativeLength::new(radius * record_scale)?;
                    let half_angle = Angle::new(half_angle * record_angle_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                        cadmpeg_ir::geometry::analytic::ConeSurface::new(
                            origin,
                            frame,
                            radius,
                            PositiveReal::ONE,
                            half_angle,
                        ),
                    )))
                }),
            LeafSurfaceEntity::Spherical => placement
                .zip(named_parameter(record, "SPHERICAL_SURFACE", 2).and_then(Value::number))
                .and_then(|((center, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = NonZeroLength::new(radius * record_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        cadmpeg_ir::geometry::analytic::SphereSurface::new(center, frame, radius),
                    )))
                }),
            LeafSurfaceEntity::Toroidal | LeafSurfaceEntity::DegenerateToroidal => placement
                .zip(named_parameter(record, surface_type, 2).and_then(Value::number))
                .zip(named_parameter(record, surface_type, 3).and_then(Value::number))
                .and_then(
                    |(((center, axis, ref_direction), major_radius), minor_radius)| {
                        let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                        let major_radius = PositiveLength::new(major_radius * record_scale)?;
                        let minor_radius = NonZeroLength::new(minor_radius * record_scale)?;
                        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                            cadmpeg_ir::geometry::analytic::TorusSurface::new(
                                center,
                                frame,
                                major_radius,
                                minor_radius,
                            ),
                        )))
                    },
                ),
            LeafSurfaceEntity::BSplineWithKnots
            | LeafSurfaceEntity::UniformSurface
            | LeafSurfaceEntity::QuasiUniformSurface
            | LeafSurfaceEntity::BezierSurface => {
                nurbs_surface(id, record, &points, &mut losses, ctx)?
                    .map(SolvedSurfaceGeometry::Nurbs)
                    .map(SurfaceGeometry::Solved)
            }
        };
        if let Some(geometry) = geometry {
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry,
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("{surface_type} #{id} has invalid geometry")),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_SURFACE_WITH_KNOTS") {
        if record.partial("B_SPLINE_SURFACE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_SURFACE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_surface(id, record, &points, &mut losses, ctx)? {
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_SURFACE_WITH_KNOTS #{id} has invalid geometry"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    // Surface constructors form the same kind of dependency graph as curves.
    // Resolve replicas in the same fixpoint as trims, bounded surfaces, and
    // offsets so a forward or nested replica cannot become an opaque carrier.
    carrier_index.refresh_surfaces(ir, ctx)?;
    let mut deferred_surface_ids = Vec::new();
    for (id, _) in exchange.entities_any(&[
        "CURVE_BOUNDED_SURFACE",
        "OFFSET_SURFACE",
        "RECTANGULAR_TRIMMED_SURFACE",
        "SURFACE_REPLICA",
    ]) {
        ctx.push_vec(&mut deferred_surface_ids, id, "step_deferred_surface_ids")?;
    }
    let mut deferred_surface_queue = VecDeque::from(deferred_surface_ids);
    let mut surface_waiting_on = HashMap::<u64, Vec<u64>>::new();
    while let Some(id) = deferred_surface_queue.pop_front() {
        if carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let resolved = if record.partial("RECTANGULAR_TRIMMED_SURFACE").is_some() {
            let Some(parameters) = entity_parameters(record, "RECTANGULAR_TRIMMED_SURFACE") else {
                continue;
            };
            let record_scale = unit_scales.length([id]).get();
            let record_angle_scale = unit_scales.angle([id]).get();
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(mut parameter_ranges) = parameters
                .get(2)
                .and_then(Value::number)
                .zip(parameters.get(3).and_then(Value::number))
                .zip(
                    parameters
                        .get(4)
                        .and_then(Value::number)
                        .zip(parameters.get(5).and_then(Value::number)),
                )
                .map(|((u1, u2), (v1, v2))| [[u1, u2], [v1, v2]])
            else {
                continue;
            };
            let Some((u_sense, v_sense)) = parameters
                .get(6)
                .and_then(Value::logical)
                .zip(parameters.get(7).and_then(Value::logical))
            else {
                continue;
            };
            if !parameter_ranges
                .iter()
                .flatten()
                .all(|parameter| parameter.is_finite())
                || parameter_ranges[0][0] == parameter_ranges[0][1]
                || parameter_ranges[1][0] == parameter_ranges[1][1]
            {
                continue;
            }
            let Some(geometry) = carrier_index
                .surfaces
                .get(&support_step)
                .and_then(|index| ir.model.surfaces.get(index.0))
                .map(|surface| &surface.geometry)
            else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let Some(solved_geometry) = geometry.solved() else {
                continue;
            };
            let Some(parameter_scales) = surface_parameter_scales_for_step(
                ir,
                &SurfaceId::from(ids::data(kind!("surface"), support_step)),
                geometry,
                record_scale,
                record_angle_scale,
                &source_curve_parameter_scales,
                ctx,
            )?
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                    "RECTANGULAR_TRIMMED_SURFACE #{id} has no established support parameterization"
                )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            for (range, parameter_scale) in parameter_ranges.iter_mut().zip(parameter_scales) {
                range[0] *= parameter_scale;
                range[1] *= parameter_scale;
            }
            for ((range, sense), domain) in parameter_ranges
                .iter_mut()
                .zip([u_sense, v_sense])
                .zip(surface_periodic_domains(solved_geometry))
            {
                if let Some(domain) = domain {
                    if sense && range[1] < range[0] {
                        range[1] = shift_periodic_parameter(range[1], domain);
                    } else if !sense && range[0] < range[1] {
                        range[0] = shift_periodic_parameter(range[0], domain);
                    }
                }
            }
            let [[u_start, u_end], [v_start, v_end]] = parameter_ranges;
            let [Some(u_start), Some(u_end), Some(v_start), Some(v_end)] = [
                FiniteReal::new(u_start),
                FiniteReal::new(u_end),
                FiniteReal::new(v_start),
                FiniteReal::new(v_end),
            ] else {
                continue;
            };
            let parameter_ranges = [[u_start, u_end], [v_start, v_end]];
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let copied_geometry =
                solved_geometry.try_clone_for_decode(ctx, "step_trim_surface_carrier")?;
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = procedural_admission.add_surface(&mut ir.model, &surface, match (|| {
                    let ranges = parameter_ranges.map(|range| {
                        DirectedParameterRange::from_finite_endpoints(range).map_err(|_| {
                            ProceduralGeometryError::Payload(
                                "surface subset ranges are not finite and non-zero",
                            )
                        })
                    });
                    let [u_range, v_range] = ranges;
                    Ok::<_, ProceduralGeometryError>(
                        cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction::from_parts(
                            SurfaceId::from(ids::data(kind!("surface"), support_step)),
                            [u_range?, v_range?],
                            Some(u_sense),
                            Some(v_sense),
                            None,
                        ),
                    )
                })()
                .map(|admitted_payload| {
                    ProceduralSurface::new(
                        ProceduralSurfaceId::from(ids::construction(
                            kind!("rectangular_trimmed_surface"),
                            id,
                        )),
                        ProceduralSurfaceDefinition::Subset(admitted_payload),
                        None,
                    )
                }) {
                    Ok(surface) => surface,
                    Err(error) => {
                        ctx.push_vec(&mut losses, StepLossCode::DecodeWarning
                                .note(format!("procedural surface #{id}: {error}")), "step_geometry_losses")?;
                        continue;
                    }
                })?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                SurfaceIndex(ir.model.surfaces.len() - 1),
                "step_geometry_surface_index",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial("CURVE_BOUNDED_SURFACE").is_some() {
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let Some(parameters) = entity_parameters(record, "CURVE_BOUNDED_SURFACE") else {
                continue;
            };
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(support_index) = carrier_index.surfaces.get(&support_step).copied() else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let support = SurfaceId::from(ids::data(kind!("surface"), support_step));
            let boundary_steps = parameters
                .get(2)
                .and_then(Value::list)
                .filter(|values| values.iter().all(|value| value.reference().is_some()));
            let mut boundaries = boundary_steps.map(|_| Vec::new());
            if let (Some(values), Some(boundaries)) = (boundary_steps, boundaries.as_mut()) {
                for boundary in values.iter().filter_map(Value::reference) {
                    ctx.push_vec(
                        boundaries,
                        CurveId::from(ids::data(kind!("curve"), boundary)),
                        "step_curve_bounded_boundaries",
                    )?;
                }
            }
            let mut boundary_pcurve_set = BTreeSet::new();
            for pcurve in boundary_steps
                .into_iter()
                .flatten()
                .filter_map(Value::reference)
                .flat_map(|boundary| boundary_pcurve_steps(boundary, support_step, exchange))
            {
                ctx.insert_btree_set(
                    &mut boundary_pcurve_set,
                    PcurveId::from(ids::data(kind!("pcurve"), pcurve)),
                    "step_curve_bounded_pcurve_set",
                )?;
            }
            let mut boundary_pcurves = Vec::new();
            for pcurve in boundary_pcurve_set {
                ctx.push_vec(&mut boundary_pcurves, pcurve, "step_curve_bounded_pcurves")?;
            }
            let implicit_outer = parameters.get(3).and_then(Value::logical);
            let Some((boundaries, implicit_outer, geometry)) = ir
                .model
                .surfaces
                .get(support_index.0)
                .and_then(|surface| surface.geometry.solved())
                .zip(boundaries)
                .zip(implicit_outer)
                .map(|((geometry, boundaries), implicit_outer)| {
                    (boundaries, implicit_outer, geometry)
                })
                .filter(|(boundaries, _, _)| {
                    !boundaries.is_empty()
                        && boundaries.iter().all(|curve| {
                            step_instance_id(curve.as_str())
                                .is_some_and(|id| carrier_index.curves.contains_key(&id))
                        })
                })
            else {
                continue;
            };
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            let copied_geometry =
                geometry.try_clone_for_decode(ctx, "step_curve_bounded_surface_carrier")?;
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = procedural_admission.add_surface(
                &mut ir.model,
                &surface,
                ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(
                        kind!("curve_bounded_surface"),
                        id,
                    )),
                    ProceduralSurfaceDefinition::CurveBounded {
                        support,
                        boundaries,
                        boundary_pcurves,
                        implicit_outer,
                    },
                    None,
                ),
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial("OFFSET_SURFACE").is_some() {
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let Some(parameters) = entity_parameters(record, "OFFSET_SURFACE") else {
                continue;
            };
            let record_scale = unit_scales.length([id]).get();
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            if !carrier_index.surfaces.contains_key(&support_step) {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            }
            let support = SurfaceId::from(ids::data(kind!("surface"), support_step));
            let distance = parameters.get(2).and_then(Value::number);
            let self_intersect = parameters
                .get(3)
                .and_then(|value| logical_value(value).ok());
            let Some((distance, self_intersect)) = distance.zip(self_intersect) else {
                continue;
            };
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = procedural_admission.add_surface(&mut ir.model, &surface, match cadmpeg_ir::geometry::surface_payloads::ParallelOffsetSurfaceConstruction::try_new(support, distance * record_scale, self_intersect).map(|admitted_payload| ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(kind!("offset_surface"), id)),
                    ProceduralSurfaceDefinition::ParallelOffset(admitted_payload),
                    None,
                )) {
                    Ok(surface) => surface,
                    Err(error) => {
                        ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!("procedural surface #{id}: {error}")), "step_geometry_losses")?;
                        continue;
                    }
                })?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial("SURFACE_REPLICA").is_some() {
            let Some(parent_step) =
                named_parameter(record, "SURFACE_REPLICA", 1).and_then(Value::reference)
            else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(record, "SURFACE_REPLICA", 2).and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.surfaces.get(&parent_step).copied() else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    parent_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .surfaces
                .get(parent_index.0)
                .and_then(|surface| surface.geometry.solved())
            else {
                continue;
            };
            let basis = basis.try_clone_for_decode(ctx, "step_surface_replica_basis")?;
            let Ok(placed) =
                cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(basis), transform)
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "SURFACE_REPLICA #{id} nests past the admitted inline basis depth"
                    )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            let geometry = SolvedSurfaceGeometry::Transformed(placed);
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = procedural_admission.add_surface(
                &mut ir.model,
                &surface,
                ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(kind!("surface_replica"), id)),
                    ProceduralSurfaceDefinition::Replica {
                        source: SurfaceId::from(ids::data(kind!("surface"), parent_step)),
                        transform,
                    },
                    None,
                ),
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
            ctx.insert_hash_set(&mut typed, operator_step, "step_geometry_typed_ids")?;
            true
        } else {
            false
        };
        if resolved {
            wake_deferred_dependents(
                id,
                &mut surface_waiting_on,
                &mut deferred_surface_queue,
                ctx,
                "step_deferred_surface_queue",
            )?;
        }
    }
    for (id, _) in exchange.entities("SURFACE_REPLICA") {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "SURFACE_REPLICA #{id} has invalid or unresolved parent/operator"
                )),
                "step_geometry_losses",
            )?;
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    for (id, _) in exchange.entities("RECTANGULAR_TRIMMED_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                "RECTANGULAR_TRIMMED_SURFACE #{id} has invalid or unresolved basis/trim selectors"
            )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities("CURVE_BOUNDED_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CURVE_BOUNDED_SURFACE #{id} has invalid or unresolved support/boundaries"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities("OFFSET_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "OFFSET_SURFACE #{id} has invalid or unresolved support parameters"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    for record in exchange.entities("EDGE_CURVE").map(|(_, record)| record) {
        let Some(curve_step) = edge_curve_geometry_reference(record)
            .and_then(|curve| curve_carrier_record(curve, exchange))
        else {
            continue;
        };
        if !carrier_index.curves.contains_key(&curve_step) {
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: CurveId::from(ids::data(kind!("curve"), curve_step)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&curve_step)
                            .map(|record| opaque_record_id(curve_step, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained undecoded topology curve #{curve_step} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                curve_step,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for (id, _) in exchange.entities_any(&[
        "CURVE_BOUNDED_SURFACE",
        "OFFSET_SURFACE",
        "RECTANGULAR_TRIMMED_SURFACE",
    ]) {
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        if !carrier_index.surfaces.contains_key(&id) {
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained unresolved deferred surface #{id} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    for (&face_id, face) in exchange.records() {
        if !face
            .partials
            .iter()
            .any(|partial| matches!(partial.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE"))
        {
            continue;
        }
        let Some(surface_step) = face_surface_reference(face) else {
            continue;
        };
        if !carrier_index.surfaces.contains_key(&surface_step) {
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), surface_step)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&surface_step)
                            .map(|record| opaque_record_id(surface_step, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "retained undecoded face surface #{surface_step} from face #{face_id} as an unknown carrier"
            )), "step_geometry_losses")?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                surface_step,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    let mut surface_parameter_scales = BTreeMap::new();
    for surface in &ir.model.surfaces {
        let Some(id) = step_instance_id(surface.id.as_str()) else {
            continue;
        };
        if let Some(scales) = surface_parameter_scales_for_step(
            ir,
            &surface.id,
            &surface.geometry,
            unit_scales.length([id]).get(),
            unit_scales.angle([id]).get(),
            &source_curve_parameter_scales,
            ctx,
        )? {
            ctx.insert_btree_map(
                &mut surface_parameter_scales,
                id,
                scales,
                "step_surface_parameter_scales",
            )?;
        }
    }
    for (id, record) in exchange.entities("PCURVE") {
        if record.partial("PCURVE").is_none() {
            continue;
        }
        let surface_step = named_parameter(record, "PCURVE", 1).and_then(Value::reference);
        let representation = named_parameter(record, "PCURVE", 2)
            .and_then(Value::reference)
            .and_then(|representation| exchange.records().get(&representation));
        let curve_steps = representation
            .and_then(representation_items)
            .into_iter()
            .flatten();
        let mut decoded = None;
        let mut decoded_count = 0;
        for curve in curve_steps {
            if let Some(geometry) = pcurve_geometries.get(&curve) {
                decoded = Some((curve, geometry));
                decoded_count += 1;
            }
        }
        let decoded = if decoded_count == 1 { decoded } else { None };
        let Some((curve_step, (geometry, geometry_records))) = surface_step
            .filter(|surface| carrier_index.surfaces.contains_key(surface))
            .and(decoded)
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("PCURVE #{id} has no decoded surface or 2D curve")),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(scales) = surface_step.and_then(|surface| surface_parameter_scales.get(&surface))
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PCURVE #{id} has no established owning surface parameterization"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let geometry = geometry.try_clone_for_decode(ctx, "step_pcurve_carrier_copy")?;
        let Ok(geometry) = geometry.scaled_coordinates_owned(ctx, *scales)? else {
            ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "PCURVE #{id} has a 2D carrier that cannot be scaled into the owning surface parameter units"
            )), "step_geometry_losses")?;
            continue;
        };
        ctx.push_vec(
            &mut ir.model.pcurves,
            Pcurve {
                id: PcurveId::from(ids::data(kind!("pcurve"), id)),
                geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            },
            "step_geometry_ir_pcurves",
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        if let Some(representation) =
            named_parameter(record, "PCURVE", 2).and_then(Value::reference)
        {
            ctx.insert_hash_set(&mut typed, representation, "step_geometry_typed_ids")?;
        }
        ctx.insert_hash_set(&mut typed, curve_step, "step_geometry_typed_ids")?;
        for &record in geometry_records {
            ctx.insert_hash_set(&mut typed, record, "step_geometry_typed_ids")?;
        }
    }

    // Curve-bounded surfaces resolve before the PCURVE pass because their 3D
    // boundaries do not depend on parameter-space geometry. Remove candidate
    // references whose pcurve carrier did not decode.
    let mut decoded_pcurve_steps = BTreeSet::new();
    for pcurve in &ir.model.pcurves {
        if let Some(id) = step_instance_id(pcurve.id.as_str()) {
            ctx.insert_btree_set(&mut decoded_pcurve_steps, id, "step_decoded_pcurve_steps")?;
        }
    }
    for surface in &mut ir.model.procedural_surfaces {
        surface.edit_definition(|definition| {
            let ProceduralSurfaceDefinition::CurveBounded {
                boundary_pcurves, ..
            } = definition
            else {
                return;
            };
            boundary_pcurves.retain(|pcurve| {
                step_instance_id(pcurve.as_str())
                    .is_some_and(|id| decoded_pcurve_steps.contains(&id))
            });
        });
    }

    for (id, record) in exchange.entities("DEGENERATE_TOROIDAL_SURFACE") {
        let Some(select_outer) = named_parameter(record, "DEGENERATE_TOROIDAL_SURFACE", 4)
            .and_then(|value| logical_value(value).ok().flatten())
        else {
            if carrier_index.surfaces.contains_key(&id) {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "DEGENERATE_TOROIDAL_SURFACE #{id} has invalid sheet selection"
                    )),
                    "step_geometry_losses",
                )?;
            }
            continue;
        };
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        if !carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let _attached = procedural_admission.add_surface(
            &mut ir.model,
            &surface,
            ProceduralSurface::new(
                ProceduralSurfaceId::from(ids::construction(kind!("degenerate_torus"), id)),
                ProceduralSurfaceDefinition::DegenerateTorus { select_outer },
                None,
            ),
        )?;
    }

    for (&id, record) in exchange.records() {
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "LENGTH_UNIT"
                    | "NAMED_UNIT"
                    | "SI_UNIT"
                    | "CONVERSION_BASED_UNIT"
                    | "MEASURE_WITH_UNIT"
                    | "LENGTH_MEASURE_WITH_UNIT"
                    | "PLANE_ANGLE_MEASURE_WITH_UNIT"
                    | "UNCERTAINTY_MEASURE_WITH_UNIT"
                    | "GEOMETRIC_REPRESENTATION_CONTEXT"
                    | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                    | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
                    | "REPRESENTATION_CONTEXT"
            )
        }) || entity_type(record, &["SHAPE_REPRESENTATION"]).is_some()
        {
            ctx.insert_hash_set(&mut typed, id, "step_geometry_typed_ids")?;
        }
    }
    Ok((
        StageOutcome {
            value: GeometryData {
                placements,
                transformation_operators,
                units: unit_scales,
            },
            claims: typed,
            losses,
            notes: Vec::new(),
        },
        carrier_index,
    ))
}

fn decode_tessellated_curve_sets(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (&id, record) in exchange.records() {
        if record.partial("TESSELLATED_CURVE_SET").is_none() {
            continue;
        }
        let Some(coordinates_id) =
            tessellated_curve_parameter(record, 0).and_then(ValueExt::reference)
        else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has no COORDINATES_LIST reference"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(coordinates_record) = exchange.records().get(&coordinates_id) else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                "TESSELLATED_CURVE_SET #{id} references missing COORDINATES_LIST #{coordinates_id}"
            )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let scale = unit_scales.length([coordinates_id]).get();
        let Some(vertices) = coordinate_rows(coordinates_record, scale, ctx)? else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has invalid COORDINATES_LIST #{coordinates_id}"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(strips) =
            tessellated_line_strips(tessellated_curve_parameter(record, 1), vertices.len(), ctx)?
        else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has invalid line strips"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let source_name = representation_item_name(record)
            .map(|value| {
                super::decode_text_charged(
                    exchange,
                    value,
                    losses,
                    id,
                    "tessellated curve name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .filter(|name| !name.is_empty());
        for (strip_index, indices) in strips.into_iter().enumerate() {
            let curve_key = if strip_index == 0 {
                IdentityKey::from(id)
            } else {
                IdentityKey::from(id)
                    .dash(key_word!("strip"))
                    .dash(strip_index)
            };
            let mut points = Vec::new();
            for index in indices {
                ctx.push_vec(&mut points, vertices[index], "step_curve_strip_points")?;
            }
            let Ok(points) = points.try_into() else {
                continue;
            };
            let Some(polyline) = PolylineCurve::from_checked_samples(
                PolylineSamples::Unparameterized { points },
                0.0,
                ctx,
            )?
            .ok() else {
                continue;
            };
            let source_name = source_name
                .as_ref()
                .map(|name| {
                    ctx.format_retained(format_args!("{name}"), "step_curve_strip_source_name")
                })
                .transpose()?;
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    parameter_range: None,
                    id: CurveId::from(ids::data(kind!("curve"), curve_key)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)),
                    source_object: Some(super::step_source_association(id, source_name)),
                },
                "step_geometry_ir_curves",
            )?;
        }
        for source_id in [id, coordinates_id] {
            ctx.insert_hash_set(typed, source_id, "step_geometry_typed_ids")?;
        }
    }
    Ok(())
}

fn tessellated_curve_parameter(record: &RawRecord, index: usize) -> Option<&Value> {
    let partial = record.partial("TESSELLATED_CURVE_SET")?;
    let offset = usize::from(record.partials.len() == 1);
    partial.parameters.get(index + offset)
}

fn tessellated_line_strips(
    value: Option<&Value>,
    point_count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    let Some(strips) = value.and_then(Value::list) else {
        return Ok(None);
    };
    if strips.is_empty() {
        return Ok(None);
    }
    let mut decoded = Vec::new();
    for strip in strips {
        let Some(values) = strip.list() else {
            return Ok(None);
        };
        if values.len() < 2 {
            return Ok(None);
        }
        let mut indices = Vec::new();
        for value in values {
            let Some(index) = value
                .integer()
                .and_then(|value| usize::try_from(value).ok())
                .and_then(|value| value.checked_sub(1))
            else {
                return Ok(None);
            };
            if index >= point_count {
                return Ok(None);
            }
            ctx.push_vec(&mut indices, index, "step_curve_strip_indices")?;
        }
        ctx.push_vec(&mut decoded, indices, "step_curve_strips")?;
    }
    Ok(Some(decoded))
}

fn face_surface_reference(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1
        && matches!(record.simple_name(), Some("ADVANCED_FACE" | "FACE_SURFACE"))
    {
        return record.parameter(2).and_then(Value::reference);
    }
    record
        .partials
        .iter()
        .filter(|partial| matches!(partial.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE"))
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(Value::reference)
        .next_back()
}

pub(super) fn associate_free_geometric_set_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for set in exchange.records().values() {
        let Some(set_type) = entity_type(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(members) = named_parameter(set, set_type, 1).and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            let name = exchange
                .records()
                .get(&member)
                .and_then(representation_item_name)
                .map(|value| {
                    super::decode_text_charged(
                        exchange,
                        value,
                        losses,
                        member,
                        "geometric-set member name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .filter(|name| !name.is_empty());
            if let Some(index) = index.curves.get(&member) {
                if owned.curves.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.curves[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
            if let Some(index) = index.points.get(&member).map(|point| &point.index) {
                if owned.points.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.points[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
            if let Some(index) = index.surfaces.get(&member) {
                if owned.surfaces.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.surfaces[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
        }
    }
    Ok(())
}

/// Associate carriers that are listed directly by a STEP representation but
/// are not owned by committed topology. A representation item is an explicit
/// source owner for free geometry; without this association the generic IR
/// reachability check would misclassify a valid standalone carrier as an
/// orphan.
pub(super) fn associate_free_representation_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for representation in exchange.records().values().filter(|record| {
        record
            .partials
            .iter()
            .any(|partial| super::representation::is_representation_name(&partial.name))
    }) {
        let Some(items) = representation_items(representation) else {
            continue;
        };
        for member in items {
            let source_name = exchange
                .records()
                .get(&member)
                .and_then(representation_item_name)
                .map(|value| {
                    super::decode_text_charged(
                        exchange,
                        value,
                        losses,
                        member,
                        "representation member name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .filter(|name| !name.is_empty());
            if let Some(index) = index.curves.get(&member) {
                if !owned.curves.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.curves[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
            if let Some(index) = index.points.get(&member).map(|point| &point.index) {
                if !owned.points.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.points[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
            if let Some(index) = index.surfaces.get(&member) {
                if !owned.surfaces.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.surfaces[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Associate geometry that is owned by presentation records rather than by a
/// shape representation. A style is a source owner even when its style
/// assignment has no surface colour, and an annotation plane owns each
/// referenced surface used to construct that plane.
pub(super) fn associate_free_presentation_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (style_id, target) in exchange.records().iter().filter_map(|(style_id, record)| {
        super::presentation::styled_item_target(record).map(|target| (*style_id, target))
    }) {
        associate_presentation_carrier(
            exchange,
            ir,
            index,
            owned,
            (target, style_id),
            losses,
            ctx,
        )?;
    }
    for (plane_id, plane) in exchange.entities("ANNOTATION_PLANE") {
        for parameter in plane
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            for target in super::reference::references(parameter, ctx) {
                let target = target?;
                if index.surfaces.contains_key(&target) {
                    associate_presentation_carrier(
                        exchange,
                        ir,
                        index,
                        owned,
                        (target, plane_id),
                        losses,
                        ctx,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn associate_presentation_carrier(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    (target, source_id): (u64, u64),
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let name = exchange
        .records()
        .get(&target)
        .and_then(representation_item_name)
        .map(|value| {
            super::decode_text_charged(
                exchange,
                value,
                losses,
                target,
                "presentation carrier name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        })
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty());
    if let Some(index) = index.curves.get(&target) {
        if !owned.curves.contains(index) {
            attach_geometry_source(
                &mut ir.model.curves[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    if let Some(index) = index.points.get(&target).map(|point| &point.index) {
        if !owned.points.contains(index) {
            attach_geometry_source(
                &mut ir.model.points[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    if let Some(index) = index.surfaces.get(&target) {
        if !owned.surfaces.contains(index) {
            attach_geometry_source(
                &mut ir.model.surfaces[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    Ok(())
}

fn representation_items(record: &RawRecord) -> Option<impl Iterator<Item = u64> + '_> {
    let items = record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(Value::list)?;
    Some(items.iter().filter_map(Value::reference))
}

fn representation_item_name(record: &RawRecord) -> Option<&Value> {
    if record.partials.len() == 1 {
        record.parameter(0)
    } else {
        record
            .partial("REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first())
    }
}

fn entity_parameters<'a>(record: &'a RawRecord, name: &str) -> Option<&'a [Value]> {
    record
        .partial(name)
        .map(|partial| partial.parameters.as_slice())
}

fn transformation_parameter<'a>(
    record: &'a RawRecord,
    name: &str,
    index: usize,
) -> Option<&'a Value> {
    let parameters = &record.partial(name)?.parameters;
    let (attribute_count, offset) = match (name, parameters.len()) {
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 6) => (5, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 8) => (5, 3),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 5) => (4, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 7) => (4, 3),
        _ => return None,
    };
    (index < attribute_count).then(|| &parameters[offset + index])
}

/// One leaf curve carrier entity dispatched by the STEP reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafCurveEntity {
    Line,
    Circle,
    Ellipse,
    Parabola,
    Hyperbola,
    Polyline,
    BSplineWithKnots,
    UniformCurve,
    QuasiUniformCurve,
    BezierCurve,
}

impl LeafCurveEntity {
    const NAMES: &'static [&'static str] = &[
        "LINE",
        "CIRCLE",
        "ELLIPSE",
        "PARABOLA",
        "HYPERBOLA",
        "POLYLINE",
        "B_SPLINE_CURVE_WITH_KNOTS",
        "UNIFORM_CURVE",
        "QUASI_UNIFORM_CURVE",
        "BEZIER_CURVE",
    ];

    fn of(record: &RawRecord) -> Option<Self> {
        entity_type(record, Self::NAMES).and_then(|name| match name {
            "LINE" => Some(Self::Line),
            "CIRCLE" => Some(Self::Circle),
            "ELLIPSE" => Some(Self::Ellipse),
            "PARABOLA" => Some(Self::Parabola),
            "HYPERBOLA" => Some(Self::Hyperbola),
            "POLYLINE" => Some(Self::Polyline),
            "B_SPLINE_CURVE_WITH_KNOTS" => Some(Self::BSplineWithKnots),
            "UNIFORM_CURVE" => Some(Self::UniformCurve),
            "QUASI_UNIFORM_CURVE" => Some(Self::QuasiUniformCurve),
            "BEZIER_CURVE" => Some(Self::BezierCurve),
            _ => None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Line => "LINE",
            Self::Circle => "CIRCLE",
            Self::Ellipse => "ELLIPSE",
            Self::Parabola => "PARABOLA",
            Self::Hyperbola => "HYPERBOLA",
            Self::Polyline => "POLYLINE",
            Self::BSplineWithKnots => "B_SPLINE_CURVE_WITH_KNOTS",
            Self::UniformCurve => "UNIFORM_CURVE",
            Self::QuasiUniformCurve => "QUASI_UNIFORM_CURVE",
            Self::BezierCurve => "BEZIER_CURVE",
        }
    }
}

/// One leaf surface carrier entity dispatched by the STEP reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafSurfaceEntity {
    Plane,
    Cylindrical,
    Conical,
    Spherical,
    DegenerateToroidal,
    Toroidal,
    BSplineWithKnots,
    UniformSurface,
    QuasiUniformSurface,
    BezierSurface,
}

impl LeafSurfaceEntity {
    const NAMES: &'static [&'static str] = &[
        "PLANE",
        "CYLINDRICAL_SURFACE",
        "CONICAL_SURFACE",
        "SPHERICAL_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
        "TOROIDAL_SURFACE",
        "B_SPLINE_SURFACE_WITH_KNOTS",
        "UNIFORM_SURFACE",
        "QUASI_UNIFORM_SURFACE",
        "BEZIER_SURFACE",
    ];

    fn of(record: &RawRecord) -> Option<Self> {
        entity_type(record, Self::NAMES).and_then(|name| match name {
            "PLANE" => Some(Self::Plane),
            "CYLINDRICAL_SURFACE" => Some(Self::Cylindrical),
            "CONICAL_SURFACE" => Some(Self::Conical),
            "SPHERICAL_SURFACE" => Some(Self::Spherical),
            "DEGENERATE_TOROIDAL_SURFACE" => Some(Self::DegenerateToroidal),
            "TOROIDAL_SURFACE" => Some(Self::Toroidal),
            "B_SPLINE_SURFACE_WITH_KNOTS" => Some(Self::BSplineWithKnots),
            "UNIFORM_SURFACE" => Some(Self::UniformSurface),
            "QUASI_UNIFORM_SURFACE" => Some(Self::QuasiUniformSurface),
            "BEZIER_SURFACE" => Some(Self::BezierSurface),
            _ => None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Plane => "PLANE",
            Self::Cylindrical => "CYLINDRICAL_SURFACE",
            Self::Conical => "CONICAL_SURFACE",
            Self::Spherical => "SPHERICAL_SURFACE",
            Self::DegenerateToroidal => "DEGENERATE_TOROIDAL_SURFACE",
            Self::Toroidal => "TOROIDAL_SURFACE",
            Self::BSplineWithKnots => "B_SPLINE_SURFACE_WITH_KNOTS",
            Self::UniformSurface => "UNIFORM_SURFACE",
            Self::QuasiUniformSurface => "QUASI_UNIFORM_SURFACE",
            Self::BezierSurface => "BEZIER_SURFACE",
        }
    }
}

fn entity_type<'a>(record: &RawRecord, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .copied()
        .find(|name| record.partial(name).is_some())
}

fn is_apll_leader_line(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "ANNOTATION_PLACEHOLDER_LEADER_LINE"
                | "ANNOTATION_TO_ANNOTATION_LEADER_LINE"
                | "ANNOTATION_TO_MODEL_LEADER_LINE"
                | "AUXILIARY_LEADER_LINE"
        )
    })
}

fn first_named_list<'a>(
    record: &'a RawRecord,
    names: &[&str],
) -> Option<impl Iterator<Item = u64> + 'a> {
    let items = record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| partial.name == *name))
        .and_then(|partial| {
            partial
                .parameters
                .iter()
                .filter_map(Value::list)
                .find(|items| items.iter().all(|item| item.reference().is_some()))
        })?;
    Some(items.iter().filter_map(Value::reference))
}

fn vertex_point_reference(record: &RawRecord) -> Option<u64> {
    record
        .partial("VERTEX_POINT")
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

fn edge_curve_geometry_reference(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(3).and_then(Value::reference);
    }
    record
        .partial("EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

#[derive(Clone, Copy)]
enum TrimMasterRepresentation {
    Parameter,
    Cartesian,
    Unspecified,
}

struct TrimParameterContext<'a> {
    points: &'a BTreeMap<u64, FinitePoint3>,
    geometry: &'a CurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    parameter_offset: f64,
    tolerance: f64,
    master_representation: TrimMasterRepresentation,
    record_id: u64,
    losses: &'a mut Vec<LossNote>,
    ctx: &'a DecodeContext<'a>,
}

fn trimmed_curve_attributes(parameters: &[Value]) -> Option<(u64, bool, TrimMasterRepresentation)> {
    let basis = parameters.get(1).and_then(Value::reference)?;
    let sense = parameters.get(4).and_then(Value::logical)?;
    let master_representation = match parameters.get(5).and_then(Value::enumeration)? {
        "PARAMETER" => TrimMasterRepresentation::Parameter,
        "CARTESIAN" => TrimMasterRepresentation::Cartesian,
        "UNSPECIFIED" => TrimMasterRepresentation::Unspecified,
        _ => return None,
    };
    Some((basis, sense, master_representation))
}

fn surface_curve_basis(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(1).and_then(Value::reference);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
        .or_else(|| record.partial("INTERSECTION_CURVE"))
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

pub(super) struct OwnedCarriers {
    curves: HashSet<CurveIndex>,
    surfaces: HashSet<SurfaceIndex>,
    points: HashSet<PointIndex>,
}

pub(super) fn topology_owned_carriers(
    ir: &CadIr,
    index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<OwnedCarriers, CodecError> {
    let mut curves = HashSet::new();
    for curve in ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve())
        .chain(
            ir.model
                .coedges
                .iter()
                .filter_map(|coedge| coedge.use_curve.as_ref().map(|use_| &use_.curve)),
        )
        .filter_map(|curve| step_instance_id(curve.as_str()))
        .filter_map(|id| index.curves.get(&id).copied())
    {
        ctx.insert_hash_set(&mut curves, curve, "step_owned_curve_carriers")?;
    }
    let mut surfaces = HashSet::new();
    for surface in ir
        .model
        .faces
        .iter()
        .filter_map(|face| step_instance_id(face.surface.as_str()))
        .filter_map(|id| index.surfaces.get(&id).copied())
    {
        ctx.insert_hash_set(&mut surfaces, surface, "step_owned_surface_carriers")?;
    }
    let mut points = HashSet::new();
    for point in ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| step_instance_id(vertex.point.as_str()))
        .filter_map(|id| index.points.get(&id).map(|point| &point.index).copied())
    {
        ctx.insert_hash_set(&mut points, point, "step_owned_point_carriers")?;
    }
    Ok(OwnedCarriers {
        curves,
        surfaces,
        points,
    })
}

pub(super) fn associate_topology_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
) {
    for (edge_id, edge) in exchange.entities("EDGE_CURVE") {
        let Some(curve_step) = edge_curve_geometry_reference(edge)
            .and_then(|curve| curve_carrier_record(curve, exchange))
        else {
            continue;
        };
        let Some(index) = index.curves.get(&curve_step) else {
            continue;
        };
        if owned.curves.contains(index) {
            continue;
        }
        if ir.model.curves[index.0].source_object.is_none() {
            ir.model.curves[index.0].source_object =
                Some(super::step_source_association(edge_id, None));
        }
    }
    for (face_id, face) in exchange.entities_any(&["ADVANCED_FACE", "FACE_SURFACE"]) {
        let Some(surface_step) = face_surface_reference(face) else {
            continue;
        };
        let Some(index) = index.surfaces.get(&surface_step) else {
            continue;
        };
        if owned.surfaces.contains(index) {
            continue;
        }
        if ir.model.surfaces[index.0].source_object.is_none() {
            ir.model.surfaces[index.0].source_object =
                Some(super::step_source_association(face_id, None));
        }
    }
    for (vertex_id, vertex) in exchange.entities("VERTEX_POINT") {
        let Some(point_step) = vertex_point_reference(vertex) else {
            continue;
        };
        let Some(index) = index.points.get(&point_step).map(|point| &point.index) else {
            continue;
        };
        if owned.points.contains(index) {
            continue;
        }
        if ir.model.points[index.0].source_object.is_none() {
            ir.model.points[index.0].source_object =
                Some(super::step_source_association(vertex_id, None));
        }
    }
}

/// Associate the basis carrier owned by each valid replica with that replica.
///
/// A replica is the carrier used by topology, while its basis remains a
/// separate IR geometry entry because the transformed geometry stores the
/// basis inline. The basis is still a real STEP dependency and must not be
/// reported as an unowned carrier by generic IR validation.
pub(super) fn associate_replica_bases(exchange: &Exchange, ir: &mut CadIr, index: &CarrierIndex) {
    for (replica_id, record) in exchange.entities("CURVE_REPLICA") {
        let Some(parent_id) =
            named_parameter(record, "CURVE_REPLICA", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.curves.get(&parent_id).copied() else {
            continue;
        };
        if ir.model.curves[parent_index.0].source_object.is_none() {
            ir.model.curves[parent_index.0].source_object =
                Some(super::step_source_association(replica_id, None));
        }
    }
    for (replica_id, record) in exchange.entities("SURFACE_REPLICA") {
        let Some(parent_id) =
            named_parameter(record, "SURFACE_REPLICA", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.surfaces.get(&parent_id).copied() else {
            continue;
        };
        if ir.model.surfaces[parent_index.0].source_object.is_none() {
            ir.model.surfaces[parent_index.0].source_object =
                Some(super::step_source_association(replica_id, None));
        }
    }
}

/// Associate surfaces referenced only as PCURVE supports with their STEP
/// PCURVE records. The canonical pcurve stores its parameter-space geometry
/// inline, so this source association preserves reachability of the separate
/// support carrier.
pub(super) fn associate_pcurve_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut owned_pcurves = BTreeSet::new();
    for pcurve in ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.as_str()))
        .chain(
            ir.model
                .loops
                .iter()
                .flat_map(|loop_| loop_.vertex_pcurves().map(|pcurve| pcurve.pcurve.as_str())),
        )
        .chain(
            ir.model
                .procedural_surfaces
                .iter()
                .filter_map(|surface| {
                    let ProceduralSurfaceDefinition::CurveBounded {
                        boundary_pcurves, ..
                    } = surface.definition()
                    else {
                        return None;
                    };
                    Some(boundary_pcurves)
                })
                .flatten()
                .map(cadmpeg_ir::ids::PcurveId::as_str),
        )
    {
        ctx.insert_btree_set(&mut owned_pcurves, pcurve, "step_owned_pcurve_supports")?;
    }

    for (pcurve_id, record) in exchange.entities("PCURVE") {
        let pcurve_identity = ids::data(kind!("pcurve"), pcurve_id);
        if !owned_pcurves.contains(pcurve_identity.as_str()) {
            continue;
        }
        let Some(surface_id) = named_parameter(record, "PCURVE", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
            continue;
        };
        if ir.model.surfaces[surface_index.0].source_object.is_none() {
            ir.model.surfaces[surface_index.0].source_object =
                Some(super::step_source_association(pcurve_id, None));
        }
    }
    Ok(())
}

/// Associate surfaces listed by retained `SURFACE_CURVE` records.
///
/// The associated-geometry list normally contains PCURVE records, but some
/// producers write the surface carriers directly. The IR stores the 3D basis
/// curve rather than the STEP wrapper, so this pass projects the wrapper's
/// surface dependencies onto the retained IR surfaces. Only wrappers that
/// participate in topology or an explicit free-geometry/presentation owner
/// are considered; an unrelated record with the same basis curve is not an
/// ownership proof.
pub(super) fn associate_surface_curve_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let retained = retained_surface_curve_ids(exchange, index, owned, ctx)?;
    for (surface_curve_id, record) in
        exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])
    {
        if !retained.contains(&surface_curve_id) {
            continue;
        }
        if let Some(curve_index) = surface_curve_basis(record)
            .and_then(|basis| index.curves.get(&basis))
            .copied()
        {
            if ir.model.curves[curve_index.0].source_object.is_none() {
                ir.model.curves[curve_index.0].source_object =
                    Some(super::step_source_association(surface_curve_id, None));
            }
        }
        for surface_id in surface_curve_supports(record, exchange, index) {
            let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
                continue;
            };
            if ir.model.surfaces[surface_index.0].source_object.is_none() {
                ir.model.surfaces[surface_index.0].source_object =
                    Some(super::step_source_association(surface_curve_id, None));
            }
        }
    }
    Ok(())
}

fn retained_surface_curve_ids(
    exchange: &Exchange,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut retained = BTreeSet::new();

    for (_, edge) in exchange.entities("EDGE_CURVE") {
        let Some(surface_curve) = edge_curve_geometry_reference(edge) else {
            continue;
        };
        let Some(record) = exchange.records().get(&surface_curve) else {
            continue;
        };
        if !is_surface_curve_record(record) {
            continue;
        }
        let Some(basis) = surface_curve_basis(record) else {
            continue;
        };
        if index
            .curves
            .get(&basis)
            .is_some_and(|curve| owned.curves.contains(curve))
        {
            ctx.insert_btree_set(
                &mut retained,
                surface_curve,
                "step_retained_surface_curve_ids",
            )?;
        }
    }

    for set in exchange.records().values() {
        let Some(set_type) = entity_type(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(members) = named_parameter(set, set_type, 1).and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            if decoded_surface_curve(member, exchange, index) {
                ctx.insert_btree_set(&mut retained, member, "step_retained_surface_curve_ids")?;
            }
        }
    }

    for representation in exchange.records().values().filter(|record| {
        record
            .partials
            .iter()
            .any(|partial| super::representation::is_representation_name(&partial.name))
    }) {
        let Some(items) = representation_items(representation) else {
            continue;
        };
        for item in items {
            if decoded_surface_curve(item, exchange, index) {
                ctx.insert_btree_set(&mut retained, item, "step_retained_surface_curve_ids")?;
            }
        }
    }

    for record in exchange.records().values() {
        if let Some(target) = super::presentation::styled_item_target(record) {
            if decoded_surface_curve(target, exchange, index) {
                ctx.insert_btree_set(&mut retained, target, "step_retained_surface_curve_ids")?;
            }
        }
    }
    for (_, plane) in exchange.entities("ANNOTATION_PLANE") {
        for parameter in plane
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            for target in super::reference::references(parameter, ctx) {
                let target = target?;
                if decoded_surface_curve(target, exchange, index) {
                    ctx.insert_btree_set(&mut retained, target, "step_retained_surface_curve_ids")?;
                }
            }
        }
    }

    Ok(retained)
}

fn decoded_surface_curve(id: u64, exchange: &Exchange, index: &CarrierIndex) -> bool {
    let Some(record) = exchange.records().get(&id) else {
        return false;
    };
    is_surface_curve_record(record)
        && surface_curve_basis(record).is_some_and(|basis| index.curves.contains_key(&basis))
}

fn is_surface_curve_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    })
}

fn surface_curve_supports<'a>(
    record: &'a RawRecord,
    exchange: &'a Exchange,
    index: &'a CarrierIndex,
) -> impl Iterator<Item = u64> + 'a {
    surface_curve_associated_geometry(record)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .filter_map(move |associated| {
            let surface = exchange
                .records()
                .get(&associated)
                .and_then(|record| named_parameter(record, "PCURVE", 1))
                .and_then(Value::reference)
                .or_else(|| {
                    index
                        .surfaces
                        .contains_key(&associated)
                        .then_some(associated)
                });
            surface.filter(|surface| index.surfaces.contains_key(surface))
        })
}

fn surface_curve_associated_geometry(record: &RawRecord) -> Option<&[Value]> {
    let values = if record.partials.len() == 1 {
        record.parameter(2)?.list()?
    } else {
        record
            .partial("SURFACE_CURVE")
            .or_else(|| record.partial("SEAM_CURVE"))
            .or_else(|| record.partial("INTERSECTION_CURVE"))
            .and_then(|partial| partial.parameters.iter().find_map(Value::list))?
    };
    Some(values)
}

fn resolve_unit_scales(
    exchange: &Exchange,
    default_length: PositiveReal,
    default_angle: PositiveReal,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<UnitScales, CodecError> {
    let mut contexts = BTreeMap::new();
    let mut needs_length_scope = false;
    let mut needs_angle_scope = false;
    for representation in exchange
        .records()
        .values()
        .filter(|record| is_representation_record(record))
    {
        let Some(context_id) = representation_context(representation) else {
            continue;
        };
        if contexts.contains_key(&context_id) {
            continue;
        }
        let scales = context_unit_scales(context_id, exchange, ctx)?;
        needs_length_scope |= scales.0.is_some_and(|scale| scale != default_length);
        needs_angle_scope |= scales.1.is_some_and(|scale| scale != default_angle);
        ctx.insert_btree_map(
            &mut contexts,
            context_id,
            scales,
            "step_unit_context_scales",
        )?;
    }
    if !needs_length_scope && !needs_angle_scope {
        // Every declared scale is already the document default. There is no
        // scope-specific scale or conflict to propagate through geometry.
        return Ok(UnitScales {
            default_length,
            default_angle,
            length: BTreeMap::new(),
            angle: BTreeMap::new(),
        });
    }
    let mut length_candidates = BTreeMap::<u64, Option<PositiveReal>>::new();
    let mut angle_candidates = BTreeMap::<u64, Option<PositiveReal>>::new();
    for (&representation_id, representation) in exchange.records() {
        if !is_representation_record(representation) {
            continue;
        }
        let Some(context_id) = representation_context(representation) else {
            continue;
        };
        let Some(&(length, angle)) = contexts.get(&context_id) else {
            continue;
        };
        let length = length.filter(|_| needs_length_scope);
        let angle = angle.filter(|_| needs_angle_scope);
        if length.is_none() && angle.is_none() {
            continue;
        }
        if let Some(length) = length {
            add_unit_candidate(
                &mut length_candidates,
                representation_id,
                length,
                ctx,
                "step_length_candidate_groups",
            )?;
        }
        if let Some(angle) = angle {
            add_unit_candidate(
                &mut angle_candidates,
                representation_id,
                angle,
                ctx,
                "step_angle_candidate_groups",
            )?;
        }
        let Some(items) = representation_items(representation) else {
            continue;
        };
        let mut members = BTreeSet::new();
        for item in items {
            collect_unit_scope_members(item, exchange, &mut members, ctx)?;
        }
        for member in members {
            if let Some(length) = length {
                add_unit_candidate(
                    &mut length_candidates,
                    member,
                    length,
                    ctx,
                    "step_length_candidate_groups",
                )?;
            }
            if let Some(angle) = angle {
                add_unit_candidate(
                    &mut angle_candidates,
                    member,
                    angle,
                    ctx,
                    "step_angle_candidate_groups",
                )?;
            }
        }
    }
    let length =
        finalize_unit_candidates(length_candidates, default_length, "length", losses, ctx)?;
    let angle =
        finalize_unit_candidates(angle_candidates, default_angle, "plane-angle", losses, ctx)?;
    Ok(UnitScales {
        default_length,
        default_angle,
        length,
        angle,
    })
}

/// A missing key has no candidate; `None` marks a final scale conflict.
fn add_unit_candidate(
    candidates: &mut BTreeMap<u64, Option<PositiveReal>>,
    id: u64,
    scale: PositiveReal,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(1, operation)?;
    if let Some(candidate) = candidates.get_mut(&id) {
        // Every candidate is compared with the first stored scale.
        // Conflict is final; later scales cannot make the set unambiguous.
        if candidate.is_some_and(|first| !same_scale(first, scale)) {
            *candidate = None;
        }
        Ok(())
    } else {
        ctx.insert_btree_map(candidates, id, Some(scale), operation)?;
        Ok(())
    }
}

fn finalize_unit_candidates(
    candidates: BTreeMap<u64, Option<PositiveReal>>,
    default: PositiveReal,
    dimension: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PositiveReal>, CodecError> {
    let mut selected = BTreeMap::new();
    let mut ambiguous = 0;
    for (id, candidate) in candidates {
        match candidate {
            Some(scale) if scale != default => {
                ctx.insert_btree_map(&mut selected, id, scale, "step_unit_selected_scales")?;
            }
            Some(_) => {}
            None => ambiguous += 1,
        }
    }
    if ambiguous > 0 {
        ctx.push_vec(losses, StepLossCode::ConflictingRepresentationUnits.note(format!(
                "{ambiguous} geometry record(s) belong to representations with conflicting {dimension} units; source-order unit selection was not applied"
            )), "step_geometry_losses")?;
    }
    Ok(selected)
}

fn unique_scale(values: &[PositiveReal]) -> Option<PositiveReal> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| same_scale(*value, first))
        .then_some(first)
}

fn same_scale(left: PositiveReal, right: PositiveReal) -> bool {
    let left = left.get();
    let right = right.get();
    let tolerance = EPS_GEOMETRY_READ_EXACT_GEOMETRY * left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= tolerance
}

fn is_representation_record(record: &RawRecord) -> bool {
    record
        .partials
        .iter()
        .any(|partial| super::representation::is_representation_name(&partial.name))
}

fn representation_context(record: &RawRecord) -> Option<u64> {
    record
        .partials
        .iter()
        .filter(|partial| super::representation::is_representation_name(&partial.name))
        .flat_map(|partial| partial.parameters.iter().rev())
        .find_map(Value::reference)
}

fn context_unit_scales(
    id: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<(Option<PositiveReal>, Option<PositiveReal>), CodecError> {
    let Some(context) = exchange.records().get(&id) else {
        return Ok((None, None));
    };
    let Some(units) = context
        .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return Ok((None, None));
    };
    let mut length_values = Vec::new();
    let mut angle_values = Vec::new();
    for unit in units.iter().filter_map(Value::reference) {
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? {
            ctx.push_vec(&mut length_values, scale, "step_context_length_scales")?;
        }
        if let Some(scale) = unit_scale_radians(unit, exchange, &mut BTreeSet::new(), ctx)? {
            ctx.push_vec(&mut angle_values, scale, "step_context_angle_scales")?;
        }
    }
    let length = unique_scale(&length_values);
    let angle = unique_scale(&angle_values);
    Ok((length, angle))
}

fn collect_unit_scope_members(
    id: u64,
    exchange: &Exchange,
    members: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _root_depth = ctx.enter_nested("step_unit_scope_walk")?;
    let mut pending = Vec::new();
    let mut next = Some(id);
    while let Some(current) = next.take().or_else(|| pending.pop()) {
        ctx.charge_work(1, "step_unit_scope_walk")?;
        let _nested = (current != id)
            .then(|| ctx.enter_nested("step_unit_scope_walk"))
            .transpose()?;
        let Some(record) = exchange.records().get(&current) else {
            continue;
        };
        if is_unit_record(record) || is_representation_context_record(record) {
            continue;
        }
        if !ctx.insert_btree_set(members, current, "step_unit_scope_members")?
            || record.partial("PCURVE").is_some()
        {
            continue;
        }
        if let Some(mapped) = record.partial("MAPPED_ITEM") {
            // The mapping source keeps the units of its mapped representation.
            // Only the mapping target belongs to this context.
            if let Some(target) = mapped.parameters.last().and_then(Value::reference) {
                ctx.push_vec(&mut pending, target, "step_unit_scope_pending")?;
            }
            continue;
        }
        for parameter in record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
        {
            for reference in super::reference::references(parameter, ctx) {
                let reference = reference?;
                let Some(referenced) = exchange.records().get(&reference) else {
                    continue;
                };
                if is_representation_record(referenced)
                    || is_unit_record(referenced)
                    || is_representation_context_record(referenced)
                {
                    continue;
                }
                ctx.push_vec(&mut pending, reference, "step_unit_scope_pending")?;
            }
        }
    }
    Ok(())
}

fn is_unit_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "LENGTH_UNIT"
                | "PLANE_ANGLE_UNIT"
                | "SOLID_ANGLE_UNIT"
                | "NAMED_UNIT"
                | "SI_UNIT"
                | "CONVERSION_BASED_UNIT"
        )
    })
}

fn is_representation_context_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "REPRESENTATION_CONTEXT"
                | "GEOMETRIC_REPRESENTATION_CONTEXT"
                | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
        )
    })
}

fn length_scale(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    document_unit_scale(exchange, UnitScaleKind::Length, ctx)
}

fn plane_angle_scale(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    document_unit_scale(exchange, UnitScaleKind::Angle, ctx)
}

#[derive(Clone, Copy)]
enum UnitScaleKind {
    Length,
    Angle,
}

impl UnitScaleKind {
    fn partial(self) -> &'static str {
        match self {
            Self::Length => "LENGTH_UNIT",
            Self::Angle => "PLANE_ANGLE_UNIT",
        }
    }

    fn resolve(
        self,
        id: u64,
        exchange: &Exchange,
        active: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<PositiveReal>, CodecError> {
        match self {
            Self::Length => unit_scale_mm(id, exchange, active, ctx),
            Self::Angle => unit_scale_radians(id, exchange, active, ctx),
        }
    }
}

fn document_unit_scale(
    exchange: &Exchange,
    kind: UnitScaleKind,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    let dimension_partial = kind.partial();
    let mut context_scales = Vec::new();
    let mut has_context_unit = false;

    for record in exchange.records().values() {
        let Some(units) = record
            .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")
            .and_then(|partial| partial.parameters.first())
            .and_then(Value::list)
        else {
            continue;
        };
        let mut unit_ids = Vec::new();
        for id in units.iter().filter_map(Value::reference) {
            if exchange
                .records()
                .get(&id)
                .is_some_and(|unit| unit.partial(dimension_partial).is_some())
            {
                ctx.push_vec(&mut unit_ids, id, "step_document_unit_ids")?;
            }
        }
        if unit_ids.is_empty() {
            continue;
        }
        has_context_unit = true;
        let mut scales = Vec::new();
        for id in unit_ids {
            let Some(scale) = kind.resolve(id, exchange, &mut BTreeSet::new(), ctx)? else {
                return Ok(None);
            };
            ctx.push_vec(&mut scales, scale, "step_document_unit_scales")?;
        }
        let Some(scale) = unique_scale(&scales) else {
            return Ok(None);
        };
        ctx.push_vec(&mut context_scales, scale, "step_document_context_scales")?;
    }

    if has_context_unit {
        return Ok(unique_scale(&context_scales));
    }

    // STEP assigns units to representation contexts, not to the document.
    // This branch is CADIR salvage for an unscoped dimension: accept only a
    // scale to which every unit occurrence in the exchange resolves.
    let mut scales = Vec::new();
    for (&id, _) in exchange
        .records()
        .iter()
        .filter(|(_, record)| record.partial(dimension_partial).is_some())
    {
        let Some(scale) = kind.resolve(id, exchange, &mut BTreeSet::new(), ctx)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut scales, scale, "step_document_fallback_scales")?;
    }
    Ok(unique_scale(&scales))
}

pub(super) fn unit_scale_radians(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    unit_scale_radians_inner(id, exchange, active, 0, ctx)
}

fn unit_scale_radians_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    if depth >= 256 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_angle_unit_scale_walk")?;
    if active.contains(&id) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, id, "step_angle_unit_active")?;
    let result = (|| -> Result<Option<f64>, CodecError> {
        let Some(record) = exchange.records().get(&id) else {
            return Ok(None);
        };
        if let Some(unit) = record.partial("SI_UNIT") {
            if unit.parameters.get(1).and_then(Value::enumeration) == Some("RADIAN") {
                let prefix = match unit.parameters.first() {
                    Some(Value::Omitted) => Some(1.0),
                    Some(Value::Enumeration(prefix)) => si_prefix(prefix),
                    _ => None,
                };
                Ok(prefix)
            } else {
                Ok(None)
            }
        } else if let Some(unit) = record.partial("CONVERSION_BASED_UNIT") {
            let Some(factor_id) = unit.parameters.get(1).and_then(Value::reference) else {
                return Ok(None);
            };
            let Some(factor) = exchange.records().get(&factor_id) else {
                return Ok(None);
            };
            let Some(value) = record_values(factor).find_map(ValueExt::typed_number) else {
                return Ok(None);
            };
            let Some(base_id) = record_values(factor).find_map(Value::reference) else {
                return Ok(None);
            };
            Ok(
                unit_scale_radians_inner(base_id, exchange, active, depth + 1, ctx)?
                    .map(|base| value * base.get()),
            )
        } else {
            Ok(None)
        }
    })();
    active.remove(&id);
    Ok(result?.and_then(PositiveReal::new))
}

pub(super) fn unit_scale_mm(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    unit_scale_mm_inner(id, exchange, active, 0, ctx)
}

fn unit_scale_mm_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    if depth >= 256 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_length_unit_scale_walk")?;
    if active.contains(&id) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, id, "step_length_unit_active")?;
    let result = (|| -> Result<Option<f64>, CodecError> {
        let Some(record) = exchange.records().get(&id) else {
            return Ok(None);
        };
        if let Some(unit) = record.partial("SI_UNIT") {
            if unit.parameters.get(1).and_then(Value::enumeration) == Some("METRE") {
                let prefix = match unit.parameters.first() {
                    Some(Value::Omitted) => Some(1.0),
                    Some(Value::Enumeration(prefix)) => si_prefix(prefix),
                    _ => None,
                };
                Ok(prefix.map(|prefix| prefix * 1000.0))
            } else {
                Ok(None)
            }
        } else if let Some(unit) = record.partial("CONVERSION_BASED_UNIT") {
            let Some(factor_id) = unit.parameters.get(1).and_then(Value::reference) else {
                return Ok(None);
            };
            let Some(factor) = exchange.records().get(&factor_id) else {
                return Ok(None);
            };
            let Some(value) = record_values(factor).find_map(ValueExt::typed_number) else {
                return Ok(None);
            };
            let Some(base_id) = factor
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
                .find_map(Value::reference)
            else {
                return Ok(None);
            };
            Ok(
                unit_scale_mm_inner(base_id, exchange, active, depth + 1, ctx)?
                    .map(|base| value * base.get()),
            )
        } else {
            Ok(None)
        }
    })();
    active.remove(&id);
    Ok(result?.and_then(PositiveReal::new))
}

const SI_MICRO: f64 = EPS_GEOMETRY_READ_COARSE_GEOMETRY;
const SI_NANO: f64 = EPS_GEOMETRY_READ_GEOMETRY;
const SI_PICO: f64 = EPS_GEOMETRY_READ_EXACT_GEOMETRY;

fn si_prefix(prefix: &str) -> Option<f64> {
    Some(match prefix {
        "EXA" => 1e18,
        "PETA" => 1e15,
        "TERA" => 1e12,
        "GIGA" => 1e9,
        "MEGA" => 1e6,
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => SI_MICRO,
        "NANO" => SI_NANO,
        "PICO" => SI_PICO,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => return None,
    })
}

/// Resolve one `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` to the linear uncertainty
/// candidates it contributes, in millimetres, and the number of its measures
/// that did not resolve.
///
/// STEP scopes an uncertainty to its representation context, so each context
/// contributes for itself. One `distance_accuracy_value` name makes that value
/// the only contribution of the context. Every other context contributes each
/// of its resolvable length measures. `linear_uncertainty` merges the equal
/// contributions of all contexts and decides what a disagreement means.
fn context_length_uncertainties(
    context: &RawRecord,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<(Vec<PositiveLength>, usize), CodecError> {
    let Some(references) = context
        .partial("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return Ok((Vec::new(), 0));
    };
    let mut measures = Vec::new();
    let mut named_count = 0;
    let mut named_value = None;
    let mut unresolved = 0;
    for uncertainty_id in references.iter().filter_map(Value::reference) {
        let Some(measure) = exchange.records().get(&uncertainty_id) else {
            unresolved += 1;
            continue;
        };
        let Some(value) = record_values(measure).find_map(ValueExt::typed_number) else {
            unresolved += 1;
            continue;
        };
        let Some(unit) = record_values(measure).find_map(Value::reference) else {
            unresolved += 1;
            continue;
        };
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? {
            let Some(result) = PositiveLength::new(value * scale.get()) else {
                unresolved += 1;
                continue;
            };
            // The CADIR convention applies to the name attribute, not
            // the optional description attribute.
            let named_distance_accuracy = measure
                .partial("UNCERTAINTY_MEASURE_WITH_UNIT")
                .and_then(|partial| partial.parameters.get(2))
                .map(|value| string_value(value, exchange, ctx))
                .transpose()?
                .flatten()
                .is_some_and(|name| name.eq_ignore_ascii_case("distance_accuracy_value"));
            if named_distance_accuracy {
                named_count += 1;
                named_value = Some(result);
            }
            ctx.push_vec(&mut measures, result, "step_uncertainty_context_measures")?;
        } else if unit_scale_radians(unit, exchange, &mut BTreeSet::new(), ctx)?.is_none() {
            unresolved += 1;
        }
    }

    if named_count == 1 {
        measures.clear();
        if let Some(value) = named_value {
            measures.push(value);
        }
    }
    Ok((measures, unresolved))
}

/// The document projection of the per-context linear uncertainty candidates.
enum LinearUncertainty {
    /// One distinct candidate, in millimetres.
    Value(PositiveLength),
    /// No candidate, with the number of measures that did not resolve.
    Empty { unresolved: usize },
    /// Several distinct candidates in millimetres, sorted and without
    /// duplicates, with the number of measures that did not resolve.
    Ambiguous {
        first: PositiveLength,
        second: PositiveLength,
        rest: Vec<PositiveLength>,
        unresolved: usize,
    },
}

fn linear_uncertainty(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<LinearUncertainty, CodecError> {
    let mut candidates: Vec<PositiveLength> = Vec::new();
    let mut unresolved = 0;
    for (_, context) in exchange.entities("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
        let (context_candidates, context_unresolved) =
            context_length_uncertainties(context, exchange, ctx)?;
        unresolved += context_unresolved;
        for candidate in context_candidates {
            // Exact equality: the candidates come from one file, so equal
            // declarations corroborate each other and are not a conflict.
            if !candidates.contains(&candidate) {
                ctx.push_vec(
                    &mut candidates,
                    candidate,
                    "step_uncertainty_distinct_candidates",
                )?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut candidates,
        |left, right| left.get().total_cmp(&right.get()),
        |_| 0,
        "step_uncertainty_candidate_sort",
    )?;

    Ok(match candidates.len() {
        0 => LinearUncertainty::Empty { unresolved },
        1 => LinearUncertainty::Value(candidates[0]),
        _ => {
            ctx.charge_work(
                u64_from_index(candidates.len()),
                "step_uncertainty_projection",
            )?;
            let first = candidates.remove(0);
            ctx.charge_work(
                u64_from_index(candidates.len()),
                "step_uncertainty_projection",
            )?;
            let second = candidates.remove(0);
            LinearUncertainty::Ambiguous {
                first,
                second,
                rest: candidates,
                unresolved,
            }
        }
    })
}

fn string_value(
    value: &Value,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    match exchange.decode_string_with_context(bytes, ctx) {
        Ok(value) => Ok(Some(value)),
        Err(crate::strings::StringDecodeFailure::Invalid(_)) => Ok(None),
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn trim_parameter(
    value: &Value,
    context: &mut TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let (parameter, cartesian) = match value {
        Value::List(values) => (
            values.iter().find(|value| is_parameter_trim_value(value)),
            values
                .iter()
                .find(|value| matches!(value, Value::Reference(_))),
        ),
        value if is_parameter_trim_value(value) => (Some(value), None),
        Value::Reference(_) => (None, Some(value)),
        _ => (None, None),
    };
    select_trim_parameter(parameter, cartesian, context)
}

fn trimmed_curve_parameter_range(
    geometry: &CurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> [f64; 2] {
    let mut start = start;
    let mut end = end;
    // A closed STEP curve may cross its parameter seam. Move the endpoint
    // that follows the declared traversal onto the next parameter branch
    // before projecting the directed trim onto the IR's ordered interval.
    if let Some(domain) = curve_periodic_domain(geometry) {
        if sense && end < start {
            end = shift_periodic_parameter(end, domain);
        } else if !sense && start < end {
            start = shift_periodic_parameter(start, domain);
        }
    }
    let range = if sense { [start, end] } else { [end, start] };
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}

fn curve_periodic_domain(geometry: &CurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            Some([0.0, std::f64::consts::TAU])
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) if curve.periodic() => {
            let [lower, upper] = nurbs_curve_parameter_domain(curve)?.endpoints();
            (upper > lower).then_some([lower, upper])
        }
        _ => None,
    }
}

fn is_parameter_trim_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Real(_) => true,
        Value::Typed(name, _) => name == "PARAMETER_VALUE",
        _ => false,
    }
}

fn trim_parameter_value(
    value: &Value,
    context: &TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let _depth = context.ctx.enter_nested("step_trim_parameter_value_walk")?;
    let Some(geometry) = context.geometry.solved() else {
        return Ok(None);
    };
    let scale = parameter_scale(
        geometry,
        context.angle_scale,
        context.linear_parameter_scale,
        context.ctx,
    )?;
    match value {
        Value::Integer(value) => Ok(cadmpeg_core::convert::f64_from_i64(*value)
            .map(|value| scale * value + context.parameter_offset)),
        Value::Real(value) => Ok(Some(scale * value.get() + context.parameter_offset)),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => {
            trim_parameter_value(value, context)
        }
        _ => Ok(None),
    }
}

fn trim_cartesian_parameter(
    value: &Value,
    context: &TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let Value::Reference(id) = value else {
        return Ok(None);
    };
    let Some(point) = context.points.get(id) else {
        return Ok(None);
    };
    let Some(geometry) = context.geometry.solved() else {
        return Ok(None);
    };
    curve_parameter_at_point(context.ctx, geometry, point.get(), context.tolerance)
}

fn select_trim_parameter(
    parameter: Option<&Value>,
    cartesian: Option<&Value>,
    context: &mut TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    match context.master_representation {
        TrimMasterRepresentation::Parameter => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                if cartesian.is_some() {
                    (context.ctx).push_vec(context.losses, StepLossCode::DecodeWarning.note(format!(
                            "TRIMMED_CURVE #{} fell back to a Cartesian trim selector because master_representation is .PARAMETER.",
                            context.record_id
                        )), "step_trim_parameter_fallback_losses")?;
                }
                match cartesian {
                    Some(value) => trim_cartesian_parameter(value, context),
                    None => Ok(None),
                }
            }
        }
        TrimMasterRepresentation::Cartesian => {
            if let Some(value) = cartesian {
                trim_cartesian_parameter(value, context)
            } else {
                if parameter.is_some() {
                    (context.ctx).push_vec(context.losses, StepLossCode::DecodeWarning.note(format!(
                            "TRIMMED_CURVE #{} fell back to a parameter trim selector because master_representation is .CARTESIAN.",
                            context.record_id
                        )), "step_trim_parameter_fallback_losses")?;
                }
                match parameter {
                    Some(value) => trim_parameter_value(value, context),
                    None => Ok(None),
                }
            }
        }
        TrimMasterRepresentation::Unspecified => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                match cartesian {
                    Some(value) => trim_cartesian_parameter(value, context),
                    None => Ok(None),
                }
            }
        }
    }
}

fn parameter_scale(
    geometry: &SolvedCurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let _depth = ctx.enter_nested("step_trim_parameter_scale_walk")?;
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => Ok(angle_scale),
        SolvedCurveGeometry::Line(_) => Ok(linear_parameter_scale),
        SolvedCurveGeometry::Transformed(placed) => {
            parameter_scale(placed.basis(), angle_scale, linear_parameter_scale, ctx)
        }
        SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Polyline(_)
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Ok(1.0),
    }
}

fn line_parameter_scale(
    exchange: &Exchange,
    curve: u64,
    length_scale: PositiveReal,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<PositiveReal, CodecError> {
    fn inherited_parent(record: &RawRecord) -> Option<u64> {
        if record.partial("CURVE_REPLICA").is_some() {
            return named_parameter(record, "CURVE_REPLICA", 1).and_then(ValueExt::reference);
        }
        if record.partial("TRIMMED_CURVE").is_some() {
            return named_parameter(record, "TRIMMED_CURVE", 1).and_then(ValueExt::reference);
        }
        if record.partial("OFFSET_CURVE_3D").is_some() {
            return named_parameter(record, "OFFSET_CURVE_3D", 1).and_then(ValueExt::reference);
        }
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            )
        }) {
            return surface_curve_basis(record);
        }
        None
    }

    fn resolve(
        exchange: &Exchange,
        curve: u64,
        length_scale: PositiveReal,
        losses: &mut Vec<LossNote>,
        visiting: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<PositiveReal, CodecError> {
        if visiting.contains(&curve) {
            return Ok(length_scale);
        }
        let _depth = ctx.enter_nested("step_line_parameter_scale_walk")?;
        ctx.insert_btree_set(visiting, curve, "step_line_parameter_scale_active")?;
        let Some(record) = exchange.records().get(&curve) else {
            visiting.remove(&curve);
            return Ok(length_scale);
        };
        let result = if record.partial("LINE").is_some() {
            if let Some(scale) = named_parameter(record, "LINE", 2)
                .and_then(ValueExt::reference)
                .and_then(|vector| exchange.records().get(&vector))
                .filter(|record| record.partial("VECTOR").is_some())
                .and_then(|record| named_parameter(record, "VECTOR", 2))
                .and_then(ValueExt::number)
                .and_then(PositiveReal::new)
                .and_then(|magnitude| PositiveReal::new(magnitude.get() * length_scale.get()))
            {
                Ok(scale)
            } else {
                ctx.push_vec(losses, StepLossCode::LineParameterScaleUnresolved.note(format!(
                        "LINE #{curve} parameter scale did not resolve; the document length scale was used"
                    )), "step_line_parameter_scale_losses")?;
                Ok(length_scale)
            }
        } else if let Some(parent) = inherited_parent(record) {
            resolve(exchange, parent, length_scale, losses, visiting, ctx)
        } else {
            Ok(length_scale)
        };
        visiting.remove(&curve);
        result
    }

    resolve(
        exchange,
        curve,
        length_scale,
        losses,
        &mut BTreeSet::new(),
        ctx,
    )
}

fn orthogonal_reference(axis: UnitVector3, reference: UnitVector3) -> Option<UnitVector3> {
    let axis = axis.as_raw();
    let reference = reference.as_raw();
    let projection = axis.dot(*reference);
    normalize(Vector3::new(
        reference.x - projection * axis.x,
        reference.y - projection * axis.y,
        reference.z - projection * axis.z,
    ))
}

fn first_projected_axis(axis: UnitVector3) -> Option<UnitVector3> {
    project_axis(default_reference_axis(axis), axis)
}

fn curve_parameter_at_point(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
    point: Point3,
    tolerance: f64,
) -> Result<Option<f64>, CodecError> {
    let offset =
        |origin: Point3| Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            Ok(Some(offset(origin).dot(direction)))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radial = offset(center);
            let y_axis = axis.cross(*ref_direction);
            Ok(Some(radial.dot(y_axis).atan2(radial.dot(*ref_direction))))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let radial = offset(center);
            let minor_direction = axis.cross(*major_direction);
            Ok(Some(
                (radial.dot(minor_direction) / minor_radius)
                    .atan2(radial.dot(*major_direction) / major_radius),
            ))
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            let Some(domain) = nurbs_curve_parameter_domain(curve)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            else {
                return Ok(None);
            };
            nurbs_curve_parameter_near_point(
                ctx,
                curve,
                point,
                tolerance,
                (domain[0] + domain[1]) * 0.5,
            )
            .map(|parameter| parameter.map(FiniteReal::get))
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let Some(inverse) = placed.transform().try_inverse_affine().ok() else {
                return Ok(None);
            };
            let Some(mapped) = inverse.apply_point(point) else {
                return Ok(None);
            };
            curve_parameter_at_point(ctx, placed.basis(), mapped.get(), tolerance)
        }
        _ => Ok(None),
    }
}

type CompositeCurveData = (Vec<(u64, CompositeCurveSegment)>, Option<bool>);

fn defer_geometry_dependency(
    waiting_on: &mut HashMap<u64, Vec<u64>>,
    dependency: u64,
    id: u64,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    item_operation: &'static str,
) -> Result<(), CodecError> {
    if !waiting_on.contains_key(&dependency) {
        ctx.reserve_map(waiting_on, 1, group_operation)?;
    }
    let dependents = waiting_on.entry(dependency).or_default();
    ctx.push_vec(dependents, id, item_operation)
}

fn wake_deferred_dependents(
    id: u64,
    waiting_on: &mut HashMap<u64, Vec<u64>>,
    queue: &mut VecDeque<u64>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(dependents) = waiting_on.remove(&id) {
        for dependent in dependents {
            ctx.push_back(queue, dependent, operation)?;
        }
    }
    Ok(())
}

fn composite_curve_dependencies<'a>(
    record: &'a RawRecord,
    exchange: &'a Exchange,
) -> impl Iterator<Item = u64> + 'a {
    composite_curve_parameters(record)
        .and_then(|(parameters, offset)| parameters.get(offset))
        .and_then(Value::list)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .filter_map(|segment| exchange.records().get(&segment))
        .filter_map(composite_curve_segment_parameters)
        .filter_map(|parameters| parameters.get(2).and_then(Value::reference))
        .filter_map(|curve| curve_carrier_record(curve, exchange))
}

fn composite_curve(
    record: &RawRecord,
    exchange: &Exchange,
    decoded: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CompositeCurveData>, CodecError> {
    let Some((parameters, offset)) = composite_curve_parameters(record) else {
        return Ok(None);
    };
    let Some(values) = parameters.get(offset).and_then(Value::list) else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for value in values {
        let Some(segment) = (|| {
            let id = value.reference()?;
            let record = exchange.records().get(&id)?;
            let parameters = composite_curve_segment_parameters(record)?;
            let transition = match parameters.first()?.enumeration()? {
                "DISCONTINUOUS" => CompositeCurveTransition::Discontinuous,
                "CONTINUOUS" => CompositeCurveTransition::Continuous,
                "CONTSAMEGRADIENT" => CompositeCurveTransition::ContSameGradient,
                "CONTSAMEGRADIENTSAMECURVATURE" => {
                    CompositeCurveTransition::ContSameGradientSameCurvature
                }
                _ => return None,
            };
            let curve_step = parameters.get(2)?.reference()?;
            let curve_step = curve_carrier_record(curve_step, exchange)?;
            decoded.curves.contains_key(&curve_step).then_some((
                id,
                CompositeCurveSegment {
                    curve: CurveId::from(ids::data(kind!("curve"), curve_step)),
                    same_sense: parameters.get(1)?.logical()?,
                    transition,
                },
            ))
        })() else {
            return Ok(None);
        };
        ctx.push_vec(&mut segments, segment, "step_composite_curve_segments")?;
    }
    let Some(self_intersect) = parameters
        .get(offset + 1)
        .and_then(|value| logical_value(value).ok())
    else {
        return Ok(None);
    };
    Ok(Some((segments, self_intersect)))
}

fn composite_curve_parameters(record: &RawRecord) -> Option<(&[Value], usize)> {
    ["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"]
        .into_iter()
        .find_map(|name| {
            record.partial(name).map(|partial| {
                (
                    partial.parameters.as_slice(),
                    usize::from(record.partials.len() == 1),
                )
            })
        })
}

fn composite_curve_segment_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("COMPOSITE_CURVE_SEGMENT")
        .map(|partial| partial.parameters.as_slice())
}

fn boundary_pcurve_steps(
    boundary: u64,
    support: u64,
    exchange: &Exchange,
) -> impl Iterator<Item = u64> + '_ {
    exchange
        .records()
        .get(&boundary)
        .and_then(composite_curve_parameters)
        .and_then(|(parameters, offset)| parameters.get(offset))
        .and_then(Value::list)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .filter_map(|segment| exchange.records().get(&segment))
        .filter_map(composite_curve_segment_parameters)
        .filter_map(|parameters| parameters.get(2).and_then(Value::reference))
        .filter_map(|curve| exchange.records().get(&curve))
        .filter(|curve| {
            curve.partials.iter().any(|partial| {
                matches!(
                    partial.name.as_str(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                )
            })
        })
        .flat_map(surface_curve_pcurves)
        .filter(move |pcurve| {
            exchange
                .records()
                .get(pcurve)
                .and_then(|record| named_parameter(record, "PCURVE", 1))
                .and_then(Value::reference)
                == Some(support)
        })
}

fn surface_curve_pcurves(record: &RawRecord) -> impl Iterator<Item = u64> + '_ {
    let value = if record.partials.len() == 1 {
        record.parameter(2)
    } else {
        record
            .partial("SURFACE_CURVE")
            .or_else(|| record.partial("SEAM_CURVE"))
            .or_else(|| record.partial("INTERSECTION_CURVE"))
            .and_then(|partial| partial.parameters.get(1))
    };
    value
        .and_then(Value::list)
        .filter(|values| values.iter().all(|value| value.reference().is_some()))
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
}

fn logical_value(value: &Value) -> Result<Option<bool>, ()> {
    match value {
        Value::Enumeration(value) if value == "T" => Ok(Some(true)),
        Value::Enumeration(value) if value == "F" => Ok(Some(false)),
        Value::Enumeration(value) if value == "U" => Ok(None),
        _ => Err(()),
    }
}

fn periodic_value(
    value: Option<&Value>,
    field: &str,
    record_id: u64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<bool>, CodecError> {
    let Some(value) = value.and_then(|value| logical_value(value).ok()) else {
        return Ok(None);
    };
    match value {
        Some(value) => Ok(Some(value)),
        None => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "{field} #{record_id} has UNKNOWN periodicity; decoded as non-periodic"
                )),
                "step_geometry_losses",
            )?;
            Ok(Some(false))
        }
    }
}

fn coordinate_rows(
    record: &RawRecord,
    scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    for rows in record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
    {
        let mut vertices = Vec::new();
        let mut valid = true;
        for row in rows {
            let point = (|| {
                let values = row.list()?;
                if values.len() != 3 {
                    return None;
                }
                FinitePoint3::new(Point3::new(
                    values[0].number()? * scale,
                    values[1].number()? * scale,
                    values[2].number()? * scale,
                ))
            })();
            let Some(point) = point else {
                valid = false;
                break;
            };
            ctx.push_vec(&mut vertices, point, "step_curve_coordinate_rows")?;
        }
        if valid && !vertices.is_empty() {
            return Ok(Some(vertices));
        }
    }
    Ok(None)
}

fn named_coordinates(
    record: &RawRecord,
    name: &str,
    index: usize,
    scale: f64,
) -> Option<FinitePoint3> {
    let values = named_parameter(record, name, index)?.list()?;
    if values.len() != 3 {
        return None;
    }
    let point = Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    );
    FinitePoint3::new(point)
}

fn apll_point_coordinates(
    record: &RawRecord,
    point_type: &str,
    scale: f64,
) -> Option<FinitePoint3> {
    let values = if record.partials.len() == 1 {
        named_parameter(record, point_type, 1).and_then(Value::list)
    } else {
        [
            ("CARTESIAN_POINT", 0),
            ("CARTESIAN_POINT", 1),
            (point_type, 0),
            (point_type, 1),
        ]
        .into_iter()
        .find_map(|(name, index)| named_parameter(record, name, index).and_then(Value::list))
    }?;
    if values.len() != 3 {
        return None;
    }
    let point = Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    );
    FinitePoint3::new(point)
}

fn named_coordinates2(record: &RawRecord, name: &str, index: usize) -> Option<Point2> {
    let values = named_parameter(record, name, index)?.list()?;
    if values.len() != 2 {
        return None;
    }
    Some(Point2::new(values[0].number()?, values[1].number()?))
}

fn direction2(value: Option<&Value>) -> Option<HypotDirection2> {
    let values = value?.list()?;
    if values.len() != 2 {
        return None;
    }
    HypotDirection2::normalized_with_length([values[0].number()?, values[1].number()?])
        .map(|(direction, _)| direction)
}

fn vector3(value: Option<&Value>, scale: f64) -> Option<Vector3> {
    let values = value?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

#[derive(Clone, Copy)]
enum DefaultNurbsKnotKind {
    Uniform,
    QuasiUniform,
    Bezier,
}

struct NurbsCurveDefinition {
    degree: u32,
    control_points: Vec<u64>,
    knots: KnotVector,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

fn nurbs_curve_definition(
    id: u64,
    record: &RawRecord,
    losses: &mut Vec<LossNote>,
    periodicity_field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurveDefinition>, CodecError> {
    let (base, offset) = if record.partials.len() > 1 {
        (geometry_or_none!(record.partial("B_SPLINE_CURVE")), 0)
    } else {
        let base = geometry_or_none!([
            "B_SPLINE_CURVE_WITH_KNOTS",
            "UNIFORM_CURVE",
            "QUASI_UNIFORM_CURVE",
            "BEZIER_CURVE",
        ]
        .into_iter()
        .find_map(|name| record.partial(name)));
        (base, 1)
    };
    let degree = geometry_or_none!(base
        .parameters
        .get(offset)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let control_points = geometry_or_none!(references(
        geometry_or_none!(base.parameters.get(offset + 1)),
        ctx
    )?);
    if usize::try_from(degree)
        .ok()
        .is_none_or(|degree| degree >= control_points.len())
    {
        return Ok(None);
    }
    let periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 3),
        periodicity_field,
        id,
        losses,
        ctx
    )?);
    let degree_usize = geometry_or_none!(usize::try_from(degree).ok());
    let expected_knots = geometry_or_none!(control_points
        .len()
        .checked_add(degree_usize)
        .and_then(|count| count.checked_add(1)));
    let knots = if let Some(knot_leaf) = record.partial("B_SPLINE_CURVE_WITH_KNOTS") {
        let tail = geometry_or_none!(knot_leaf.parameters.len().checked_sub(3));
        geometry_or_none!(expand_knots(
            geometry_or_none!(knot_leaf.parameters.get(tail)),
            geometry_or_none!(knot_leaf.parameters.get(tail + 1)),
            expected_knots,
            ctx,
        )?)
    } else {
        let kind = if record.partial("UNIFORM_CURVE").is_some() {
            DefaultNurbsKnotKind::Uniform
        } else if record.partial("QUASI_UNIFORM_CURVE").is_some() {
            DefaultNurbsKnotKind::QuasiUniform
        } else if record.partial("BEZIER_CURVE").is_some() {
            DefaultNurbsKnotKind::Bezier
        } else {
            return Ok(None);
        };
        geometry_or_none!(default_nurbs_knots(
            control_points.len(),
            degree,
            kind,
            ctx
        )?)
    };
    if knots.len() != expected_knots {
        return Ok(None);
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_CURVE") {
        Some(geometry_or_none!(numbers(
            geometry_or_none!(leaf.parameters.first()),
            ctx
        )?))
    } else {
        None
    };
    Ok(Some(NurbsCurveDefinition {
        degree,
        control_points,
        knots,
        weights,
        periodic,
    }))
}

fn default_nurbs_knots(
    control_point_count: usize,
    degree: u32,
    kind: DefaultNurbsKnotKind,
    ctx: &DecodeContext<'_>,
) -> Result<Option<KnotVector>, CodecError> {
    let degree = geometry_or_none!(usize::try_from(degree).ok());
    let expected = geometry_or_none!(control_point_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1)));
    let mut knots = Vec::new();
    match kind {
        DefaultNurbsKnotKind::Uniform => {
            for index in 0..expected {
                let index = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
                let degree = geometry_or_none!(cadmpeg_core::convert::f64_from_index(degree));
                let knot = geometry_or_none!(FiniteReal::new(index - degree));
                ctx.push_vec(&mut knots, knot.get(), "step_default_nurbs_knots")?;
            }
        }
        DefaultNurbsKnotKind::QuasiUniform => {
            let distinct_count = geometry_or_none!(control_point_count
                .checked_sub(degree)
                .and_then(|count| count.checked_add(1)));
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    geometry_or_none!(degree.checked_add(1))
                } else {
                    1
                };
                let knot = geometry_or_none!(FiniteReal::new(geometry_or_none!(
                    cadmpeg_core::convert::f64_from_index(index)
                )));
                for _ in 0..multiplicity {
                    ctx.push_vec(&mut knots, knot.get(), "step_default_nurbs_knots")?;
                }
            }
        }
        DefaultNurbsKnotKind::Bezier => {
            if degree == 0 {
                return Ok(None);
            }
            let segment_count = geometry_or_none!(control_point_count.checked_sub(1));
            if segment_count % degree != 0 {
                return Ok(None);
            }
            let segment_count = segment_count / degree;
            let distinct_count = geometry_or_none!(segment_count.checked_add(1));
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    geometry_or_none!(degree.checked_add(1))
                } else {
                    degree
                };
                let knot = geometry_or_none!(FiniteReal::new(geometry_or_none!(
                    cadmpeg_core::convert::f64_from_index(index)
                )));
                for _ in 0..multiplicity {
                    ctx.push_vec(&mut knots, knot.get(), "step_default_nurbs_knots")?;
                }
            }
        }
    }
    if knots.len() != expected {
        return Ok(None);
    }
    Ok(KnotVector::new(ctx, knots)?.ok())
}

fn nurbs_curve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurve>, CodecError> {
    let definition = geometry_or_none!(nurbs_curve_definition(
        id,
        record,
        losses,
        "B_SPLINE_CURVE",
        ctx
    )?);
    let mut control_points = Vec::new();
    for id in definition.control_points {
        let point = geometry_or_none!(points.get(&id).copied());
        ctx.push_vec(
            &mut control_points,
            point,
            "step_nurbs_curve_control_points",
        )?;
    }
    let curve = NurbsCurve::from_lanes(
        ctx,
        definition.degree,
        definition.knots,
        control_points,
        definition.weights,
        definition.periodic,
    )?;
    match curve {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_CURVE #{id} is not a curve carrier: {error}"
                )),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn nurbs_pcurve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PcurveGeometry>, CodecError> {
    let definition = geometry_or_none!(nurbs_curve_definition(
        id,
        record,
        losses,
        "B_SPLINE_CURVE pcurve",
        ctx
    )?);
    let mut control_points = Vec::new();
    for id in definition.control_points {
        let point = geometry_or_none!(points.get(&id).copied());
        ctx.push_vec(
            &mut control_points,
            point,
            "step_nurbs_pcurve_control_points",
        )?;
    }
    let pcurve = PcurveNurbs::from_lanes(
        ctx,
        definition.degree,
        definition.knots,
        control_points,
        definition.weights,
        definition.periodic,
    )?;
    match pcurve {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_CURVE pcurve #{id} is not a pcurve carrier: {error}"
                )),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

#[derive(Clone, Copy)]
struct PcurveSources<'a> {
    points: &'a BTreeMap<u64, Point2>,
    vectors: &'a BTreeMap<u64, Point2>,
    placements: &'a BTreeMap<u64, (Point2, HypotDirection2, HypotDirection2)>,
    transformations: &'a BTreeMap<u64, Transform2>,
    angle_scale: f64,
}

fn decode_pcurve_geometry(
    id: u64,
    exchange: &Exchange,
    sources: PcurveSources<'_>,
    losses: &mut Vec<LossNote>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(PcurveGeometry, BTreeSet<u64>)>, CodecError> {
    let PcurveSources {
        points,
        vectors,
        placements,
        transformations,
        angle_scale,
    } = sources;
    if depth >= 256 || active.contains(&id) {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_pcurve_geometry_walk")?;
    ctx.insert_btree_set(active, id, "step_pcurve_geometry_active")?;
    let result = (|| -> Result<Option<_>, CodecError> {
        let record = geometry_or_none!(exchange.records().get(&id));
        let mut records = BTreeSet::new();
        ctx.insert_btree_set(&mut records, id, "step_pcurve_source_records")?;
        let geometry = if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "B_SPLINE_CURVE_WITH_KNOTS"
                    | "UNIFORM_CURVE"
                    | "QUASI_UNIFORM_CURVE"
                    | "BEZIER_CURVE"
            )
        }) {
            geometry_or_none!(nurbs_pcurve(id, record, points, losses, ctx)?)
        } else {
            let curve_type = geometry_or_none!(entity_type(
                record,
                &[
                    "LINE",
                    "CIRCLE",
                    "ELLIPSE",
                    "PARABOLA",
                    "HYPERBOLA",
                    "POLYLINE",
                    "CURVE_REPLICA",
                    "TRIMMED_CURVE",
                    "OFFSET_CURVE_2D",
                    "UNIFORM_CURVE",
                    "QUASI_UNIFORM_CURVE",
                    "BEZIER_CURVE",
                ],
            ));
            match curve_type {
                "LINE" => {
                    let origin = geometry_or_none!(named_parameter(record, "LINE", 1)
                        .and_then(Value::reference)
                        .and_then(|point| points.get(&point).copied()));
                    let direction = geometry_or_none!(named_parameter(record, "LINE", 2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied()));
                    PcurveGeometry::Line(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction).ok()
                    ))
                }
                "CIRCLE" => {
                    let placement = geometry_or_none!(
                        named_parameter(record, "CIRCLE", 1).and_then(Value::reference)
                    );
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let radius = geometry_or_none!(
                        named_parameter(record, "CIRCLE", 2).and_then(Value::number)
                    );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Circle(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::CirclePcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(radius)),
                        )
                    ))
                }
                "ELLIPSE" => {
                    let placement = geometry_or_none!(
                        named_parameter(record, "ELLIPSE", 1).and_then(Value::reference)
                    );
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let major_radius = geometry_or_none!(
                        named_parameter(record, "ELLIPSE", 2).and_then(Value::number)
                    );
                    let minor_radius = geometry_or_none!(
                        named_parameter(record, "ELLIPSE", 3).and_then(Value::number)
                    );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Ellipse(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::EllipsePcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(major_radius)),
                            geometry_or_none!(PositiveReal::new(minor_radius)),
                        )
                    ))
                }
                "PARABOLA" => {
                    let placement = geometry_or_none!(
                        named_parameter(record, "PARABOLA", 1).and_then(Value::reference)
                    );
                    let (vertex, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let focal_distance = geometry_or_none!(
                        named_parameter(record, "PARABOLA", 2).and_then(Value::number)
                    );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Parabola(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(vertex)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(focal_distance)),
                        )
                    ))
                }
                "HYPERBOLA" => {
                    let placement = geometry_or_none!(
                        named_parameter(record, "HYPERBOLA", 1).and_then(Value::reference)
                    );
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let major_radius = geometry_or_none!(
                        named_parameter(record, "HYPERBOLA", 2).and_then(Value::number)
                    );
                    let minor_radius = geometry_or_none!(
                        named_parameter(record, "HYPERBOLA", 3).and_then(Value::number)
                    );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Hyperbola(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(major_radius)),
                            geometry_or_none!(PositiveReal::new(minor_radius)),
                        )
                    ))
                }
                "POLYLINE" => geometry_or_none!(polyline_pcurve(id, record, points, losses, ctx)?),
                "CURVE_REPLICA" => {
                    let basis_id = geometry_or_none!(
                        named_parameter(record, "CURVE_REPLICA", 1).and_then(Value::reference)
                    );
                    let operator_id =
                        geometry_or_none!(
                            named_parameter(record, "CURVE_REPLICA", 2).and_then(Value::reference)
                        );
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    let transform = geometry_or_none!(transformations.get(&operator_id).copied());
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    ctx.insert_btree_set(&mut records, operator_id, "step_pcurve_source_records")?;
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Transformed(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
                            Box::new(basis),
                            transform
                        )
                        .ok()
                    ))
                }
                "TRIMMED_CURVE" => {
                    let basis_id = geometry_or_none!(
                        named_parameter(record, "TRIMMED_CURVE", 1).and_then(Value::reference)
                    );
                    let sense = geometry_or_none!(
                        named_parameter(record, "TRIMMED_CURVE", 4).and_then(Value::logical)
                    );
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    let scale = if matches!(
                        basis,
                        PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_)
                    ) {
                        angle_scale
                    } else {
                        1.0
                    };
                    let start =
                        geometry_or_none!(named_parameter(record, "TRIMMED_CURVE", 2)
                            .and_then(pcurve_trim_parameter))
                        .get()
                            * scale;
                    let end =
                        geometry_or_none!(named_parameter(record, "TRIMMED_CURVE", 3)
                            .and_then(pcurve_trim_parameter))
                        .get()
                            * scale;
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    let (parameter_range, same_sense) =
                        trimmed_pcurve_parameterization(&basis, start, end, sense);
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Trimmed(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::TrimmedPcurve::try_new(
                            parameter_range,
                            same_sense,
                            Box::new(basis)
                        )
                        .ok()
                    ))
                }
                "OFFSET_CURVE_2D" => {
                    let basis_id =
                        geometry_or_none!(named_parameter(record, "OFFSET_CURVE_2D", 1)
                            .and_then(Value::reference));
                    let distance = geometry_or_none!(named_parameter(record, "OFFSET_CURVE_2D", 2)
                        .and_then(Value::number)
                        .and_then(FiniteReal::new));
                    geometry_or_none!(
                        named_parameter(record, "OFFSET_CURVE_2D", 3).and_then(Value::logical)
                    );
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Offset(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::OffsetPcurve::from_finite_parts(
                            distance,
                            Box::new(basis)
                        )
                        .ok()
                    ))
                }
                _ => return Ok(None),
            }
        };
        Ok(Some((geometry, records)))
    })();
    active.remove(&id);
    result
}

fn pcurve_trim_parameter(value: &Value) -> Option<FiniteReal> {
    fn bare_number(value: &Value) -> Option<f64> {
        match value {
            Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value),
            Value::Real(value) => Some(value.get()),
            _ => None,
        }
    }

    match value {
        Value::Integer(_) | Value::Real(_) => bare_number(value),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => bare_number(value),
        Value::List(values) => values
            .iter()
            .find_map(|value| match value {
                Value::Typed(name, value) if name == "PARAMETER_VALUE" => bare_number(value),
                _ => None,
            })
            .or_else(|| {
                values.iter().find_map(|value| match value {
                    Value::Integer(_) | Value::Real(_) => bare_number(value),
                    _ => None,
                })
            }),
        _ => None,
    }
    .and_then(FiniteReal::new)
}

fn trimmed_pcurve_parameterization(
    geometry: &PcurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> ([f64; 2], bool) {
    let mut start = start;
    let mut end = end;
    // Closed STEP pcurves use cyclic parameter branches. Move the endpoint
    // that follows the declared traversal before projecting to an ordered
    // basis interval; non-closed malformed input still gets a valid interval.
    if let Some(domain) = pcurve_periodic_domain(geometry) {
        if sense && end < start {
            end = shift_periodic_parameter(end, domain);
        } else if !sense && start < end {
            start = shift_periodic_parameter(start, domain);
        }
    }
    let [from, to] = if sense { [start, end] } else { [end, start] };
    if from <= to {
        ([from, to], true)
    } else {
        ([to, from], false)
    }
}

fn pcurve_periodic_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_) | PcurveGeometry::Harmonic(_) => {
            Some([0.0, std::f64::consts::TAU])
        }
        PcurveGeometry::Nurbs { nurbs } if nurbs.periodic() => {
            pcurve_nurbs_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
        }
        PcurveGeometry::PolarNurbs { nurbs } if nurbs.periodic() => {
            pcurve_nurbs_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_periodic_domain(basis)
        }
        PcurveGeometry::Transformed(placed) => pcurve_periodic_domain(placed.basis()),
        _ => None,
    }
}

fn pcurve_nurbs_parameter_domain(
    degree: u32,
    knots: &KnotVector,
    count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    (upper > lower).then_some([lower, upper])
}

fn shift_periodic_parameter(value: f64, [lower, upper]: [f64; 2]) -> f64 {
    let period = upper - lower;
    if period.is_finite() {
        value + period
    } else {
        upper + (value - lower)
    }
}

fn surface_parameter_scales_for_step(
    ir: &CadIr,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    procedural_surface_parameter_scales(
        ir,
        surface_id,
        geometry,
        length_scale,
        angle_scale,
        source_curve_parameter_scales,
        ctx,
    )
}

fn procedural_surface_parameter_scales(
    ir: &CadIr,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let _depth = ctx.enter_nested("step_surface_parameter_scale_walk")?;
    let mut active_storage = ctx.reserve_scoped(0, "step surface scale traversal storage")?;
    let mut active = BTreeSet::new();
    let mut surface_id = surface_id;
    let mut geometry = geometry;
    loop {
        if active.contains(surface_id) {
            return Ok(None);
        }
        let key = active_storage.with_storage(|| {
            ctx.format_retained(
                format_args!("{}", surface_id.as_str()),
                "step_surface_scale_active_id",
            )
        })?;
        let key = geometry_or_none!(SurfaceId::mint(key).ok());
        active_storage
            .with_storage(|| ctx.insert_btree_set(&mut active, key, "step_surface_scale_active"))?;
        let Some(solved) = geometry.solved() else {
            return Ok(None);
        };
        match surface_geometry_parameter_scales(
            ir,
            surface_id,
            solved,
            length_scale,
            angle_scale,
            source_curve_parameter_scales,
            ctx,
        )? {
            SurfaceScaleStep::Value(scales) => return Ok(scales),
            SurfaceScaleStep::Support(support) => {
                let Some(carrier) = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *support)
                else {
                    return Ok(None);
                };
                surface_id = support;
                geometry = &carrier.geometry;
            }
        }
    }
}

enum SurfaceScaleStep<'a> {
    Value(Option<[f64; 2]>),
    Support(&'a SurfaceId),
}

fn surface_geometry_parameter_scales<'a>(
    ir: &'a CadIr,
    surface_id: &SurfaceId,
    geometry: &SolvedSurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<SurfaceScaleStep<'a>, CodecError> {
    let _depth = ctx.enter_nested("step_surface_geometry_scale_walk")?;
    let mut geometry = geometry;
    while let SolvedSurfaceGeometry::Transformed(placed) = geometry {
        geometry = placed.basis();
    }
    let scales = match geometry {
        SolvedSurfaceGeometry::Plane(_) => Some([length_scale, length_scale]),
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => {
            Some([angle_scale, length_scale])
        }
        SolvedSurfaceGeometry::Sphere(_) | SolvedSurfaceGeometry::Torus(_) => {
            Some([angle_scale, angle_scale])
        }
        SolvedSurfaceGeometry::Nurbs(_) => Some([1.0, 1.0]),
        SolvedSurfaceGeometry::Transformed(_) => None,
        SolvedSurfaceGeometry::Unknown { .. } => {
            let mut candidates = ir.model.procedural_surfaces.iter().filter(|procedural| {
                ir.model.procedural_surface_owner(&procedural.id) == Some(surface_id)
            });
            let Some(procedural) = candidates.next() else {
                return Ok(SurfaceScaleStep::Value(None));
            };
            if candidates.next().is_some() {
                return Ok(SurfaceScaleStep::Value(None));
            }
            if let Some(support) = procedural_surface_support(procedural.definition()) {
                return Ok(SurfaceScaleStep::Support(support));
            }
            return procedural_definition_parameter_scales(
                ir,
                procedural.definition(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )
            .map(SurfaceScaleStep::Value);
        }
        SolvedSurfaceGeometry::Polygonal(_) => None,
    };
    Ok(SurfaceScaleStep::Value(scales))
}

fn procedural_surface_support(definition: &ProceduralSurfaceDefinition) -> Option<&SurfaceId> {
    match definition {
        ProceduralSurfaceDefinition::Offset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::ParallelOffset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::Subset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::SubSurface(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => Some(support),
        ProceduralSurfaceDefinition::Replica { source, .. } => Some(source),
        _ => None,
    }
}

fn procedural_definition_parameter_scales(
    ir: &CadIr,
    definition: &ProceduralSurfaceDefinition,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    match definition {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            Ok(Some([
                geometry_or_none!(directrix_parameter_scale(
                    ir,
                    directrix,
                    length_scale,
                    angle_scale,
                    source_curve_parameter_scales,
                    ctx,
                )?),
                1.0,
            ]))
        }
        ProceduralSurfaceDefinition::LinearSweep(definition_payload) => Ok(Some([
            geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?),
            1.0,
        ])),
        ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => Ok(Some([
            angle_scale,
            geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?),
        ])),
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?);
            Ok(Some(if *definition_payload.transposed() {
                [angle_scale, directrix]
            } else {
                [directrix, angle_scale]
            }))
        }
        ProceduralSurfaceDefinition::DegenerateTorus { .. } => Ok(Some([angle_scale, angle_scale])),
        _ => Ok(None),
    }
}

fn directrix_parameter_scale(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    if let Some(source_scale) =
        step_instance_id(curve_id.as_str()).and_then(|id| source_curve_parameter_scales.get(&id))
    {
        return Ok(Some(source_scale.get()));
    }
    directrix_parameter_scale_inner(
        ir,
        curve_id,
        length_scale,
        angle_scale,
        &mut BTreeSet::new(),
        ctx,
    )
}

fn directrix_parameter_scale_inner(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    active: &mut BTreeSet<CurveId>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    if active.contains(curve_id) {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_directrix_scale_walk")?;
    let key = ctx.format_retained(
        format_args!("{}", curve_id.as_str()),
        "step_directrix_scale_active_id",
    )?;
    let key = geometry_or_none!(CurveId::mint(key).ok());
    ctx.insert_btree_set(active, key, "step_directrix_scale_active")?;
    let scale = if let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) {
        if let Some(solved) = curve.geometry.solved() {
            directrix_geometry_parameter_scale(solved, length_scale, angle_scale, ctx)
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    };
    active.remove(curve_id);
    scale
}

fn directrix_geometry_parameter_scale(
    geometry: &SolvedCurveGeometry,
    length_scale: f64,
    angle_scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let _depth = ctx.enter_nested("step_directrix_geometry_scale_walk")?;
    match geometry {
        SolvedCurveGeometry::Line(_) => Ok(Some(length_scale)),
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => Ok(Some(angle_scale)),
        SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Polyline(_) => Ok(Some(1.0)),
        SolvedCurveGeometry::Transformed(placed) => {
            directrix_geometry_parameter_scale(placed.basis(), length_scale, angle_scale, ctx)
        }
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Ok(None),
    }
}

pub(super) fn surface_periodic_domains(geometry: &SolvedSurfaceGeometry) -> [Option<[f64; 2]>; 2] {
    match geometry {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [Some([0.0, std::f64::consts::TAU]), None],
        SolvedSurfaceGeometry::Torus(_) => [
            Some([0.0, std::f64::consts::TAU]),
            Some([0.0, std::f64::consts::TAU]),
        ],
        SolvedSurfaceGeometry::Nurbs(surface) => [
            surface
                .u_periodic()
                .then(|| {
                    nurbs_surface_parameter_domain(
                        surface.u_degree(),
                        surface.u_knots(),
                        surface.u_count(),
                    )
                })
                .flatten(),
            surface
                .v_periodic()
                .then(|| {
                    nurbs_surface_parameter_domain(
                        surface.v_degree(),
                        surface.v_knots(),
                        surface.v_count(),
                    )
                })
                .flatten(),
        ],
        SolvedSurfaceGeometry::Transformed(placed) => surface_periodic_domains(placed.basis()),
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [None, None],
    }
}

fn nurbs_surface_parameter_domain(
    degree: u32,
    knots: &KnotVector,
    count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    (upper > lower).then_some([lower, upper])
}

fn polyline_pcurve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PcurveGeometry>, CodecError> {
    let values = geometry_or_none!(record.parameter(1).and_then(Value::list));
    let mut control_points = Vec::new();
    for value in values {
        let point = geometry_or_none!(value.reference().and_then(|id| points.get(&id).copied()));
        ctx.push_vec(&mut control_points, point, "step_polyline_pcurve_points")?;
    }
    if control_points.len() < 2 {
        return Ok(None);
    }
    let last = geometry_or_none!(cadmpeg_core::convert::f64_from_index(
        control_points.len() - 1
    ));
    let mut knots = Vec::new();
    ctx.push_vec(&mut knots, 0.0, "step_polyline_pcurve_knots")?;
    for index in 0..control_points.len() {
        let knot = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
        ctx.push_vec(&mut knots, knot, "step_polyline_pcurve_knots")?;
    }
    ctx.push_vec(&mut knots, last, "step_polyline_pcurve_knots")?;
    match PcurveNurbs::from_lanes(ctx, 1, knots, control_points, None, false)? {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "POLYLINE pcurve #{id} is not a pcurve carrier: {error}"
                )),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn polyline(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurve>, CodecError> {
    let values = geometry_or_none!(record.parameter(1).and_then(Value::list));
    let mut control_points = Vec::new();
    for value in values {
        let point = geometry_or_none!(value
            .reference()
            .and_then(|id| points.get(&id).copied().map(FinitePoint3::get)));
        ctx.push_vec(&mut control_points, point, "step_polyline_points")?;
    }
    if control_points.len() < 2 {
        return Ok(None);
    }
    let last = geometry_or_none!(cadmpeg_core::convert::f64_from_index(
        control_points.len() - 1
    ));
    let mut knots = Vec::new();
    ctx.push_vec(&mut knots, 0.0, "step_polyline_knots")?;
    for index in 0..control_points.len() {
        let knot = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
        ctx.push_vec(&mut knots, knot, "step_polyline_knots")?;
    }
    ctx.push_vec(&mut knots, last, "step_polyline_knots")?;
    match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        ctx,
        1,
        knots,
        control_points,
        None,
        false,
    )? {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning
                    .note(format!("POLYLINE #{id} is not a curve carrier: {error}")),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn nurbs_surface(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsSurface>, CodecError> {
    let (base, offset) = if record.partials.len() > 1 {
        (geometry_or_none!(record.partial("B_SPLINE_SURFACE")), 0)
    } else {
        let base = geometry_or_none!([
            "B_SPLINE_SURFACE_WITH_KNOTS",
            "UNIFORM_SURFACE",
            "QUASI_UNIFORM_SURFACE",
            "BEZIER_SURFACE",
        ]
        .into_iter()
        .find_map(|name| record.partial(name)));
        (base, 1)
    };
    let u_degree = geometry_or_none!(base
        .parameters
        .get(offset)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let v_degree = geometry_or_none!(base
        .parameters
        .get(offset + 1)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let rows = geometry_or_none!(base.parameters.get(offset + 2).and_then(Value::list));
    if !rows.iter().all(|row| row.list().is_some()) {
        return Ok(None);
    }
    let u_count = geometry_or_none!(u32::try_from(rows.len()).ok());
    let v_count = geometry_or_none!(rows
        .first()
        .and_then(Value::list)
        .and_then(|row| u32::try_from(row.len()).ok()));
    if v_count == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || rows.iter().any(|row| {
            row.list()
                .is_none_or(|row| row.len() != cadmpeg_core::decode::index_from_u32(v_count))
        })
    {
        return Ok(None);
    }
    let mut control_points = Vec::new();
    for row in rows {
        let mut decoded_row = Vec::new();
        for value in geometry_or_none!(row.list()) {
            let point = geometry_or_none!(value
                .reference()
                .and_then(|id| points.get(&id).copied().map(FinitePoint3::get)));
            ctx.push_vec(&mut decoded_row, point, "step_nurbs_surface_control_points")?;
        }
        ctx.push_vec(&mut control_points, decoded_row, "step_nurbs_surface_rows")?;
    }
    let surface_name = [
        "B_SPLINE_SURFACE_WITH_KNOTS",
        "UNIFORM_SURFACE",
        "QUASI_UNIFORM_SURFACE",
        "BEZIER_SURFACE",
    ]
    .into_iter()
    .find(|name| record.partial(name).is_some())
    .unwrap_or("B_SPLINE_SURFACE");
    let u_periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 4),
        &format!("{surface_name} U direction"),
        id,
        losses,
        ctx,
    )?);
    let v_periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 5),
        &format!("{surface_name} V direction"),
        id,
        losses,
        ctx,
    )?);
    let expected_u = geometry_or_none!(usize::try_from(u_count)
        .ok()
        .and_then(|count| usize::try_from(u_degree)
            .ok()
            .and_then(|degree| count.checked_add(degree)))
        .and_then(|count| count.checked_add(1)));
    let expected_v = geometry_or_none!(usize::try_from(v_count)
        .ok()
        .and_then(|count| usize::try_from(v_degree)
            .ok()
            .and_then(|degree| count.checked_add(degree)))
        .and_then(|count| count.checked_add(1)));
    let (u_knots, v_knots) = if let Some(knot_leaf) = record.partial("B_SPLINE_SURFACE_WITH_KNOTS")
    {
        let tail = geometry_or_none!(knot_leaf.parameters.len().checked_sub(5));
        (
            geometry_or_none!(expand_knots(
                geometry_or_none!(knot_leaf.parameters.get(tail)),
                geometry_or_none!(knot_leaf.parameters.get(tail + 2)),
                expected_u,
                ctx,
            )?),
            geometry_or_none!(expand_knots(
                geometry_or_none!(knot_leaf.parameters.get(tail + 1)),
                geometry_or_none!(knot_leaf.parameters.get(tail + 3)),
                expected_v,
                ctx,
            )?),
        )
    } else {
        let kind = if record.partial("UNIFORM_SURFACE").is_some() {
            DefaultNurbsKnotKind::Uniform
        } else if record.partial("QUASI_UNIFORM_SURFACE").is_some() {
            DefaultNurbsKnotKind::QuasiUniform
        } else if record.partial("BEZIER_SURFACE").is_some() {
            DefaultNurbsKnotKind::Bezier
        } else {
            return Ok(None);
        };
        (
            geometry_or_none!(default_nurbs_knots(
                geometry_or_none!(usize::try_from(u_count).ok()),
                u_degree,
                kind,
                ctx
            )?),
            geometry_or_none!(default_nurbs_knots(
                geometry_or_none!(usize::try_from(v_count).ok()),
                v_degree,
                kind,
                ctx
            )?),
        )
    };
    if u_knots.len() != expected_u || v_knots.len() != expected_v {
        return Ok(None);
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_SURFACE") {
        let rows = geometry_or_none!(leaf.parameters.first().and_then(Value::list));
        let mut values = Vec::new();
        for row in rows {
            let mut decoded_row = Vec::new();
            for value in geometry_or_none!(row.list()) {
                let number = geometry_or_none!(value.number());
                ctx.push_vec(&mut decoded_row, number, "step_nurbs_surface_weight_values")?;
            }
            ctx.push_vec(&mut values, decoded_row, "step_nurbs_surface_weight_rows")?;
        }
        Some(values)
    } else {
        None
    };
    let surface = NurbsSurface::from_lanes(
        ctx,
        NurbsSurfaceAxis::new(u_degree, u_knots, u_periodic),
        NurbsSurfaceAxis::new(v_degree, v_knots, v_periodic),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, weights),
        false,
    )?;
    match surface {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_SURFACE #{id} is not a surface carrier: {error}"
                )),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn expand_knots(
    multiplicities: &Value,
    distinct: &Value,
    expected: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<KnotVector>, CodecError> {
    let multiplicities = geometry_or_none!(multiplicities.list());
    let distinct = geometry_or_none!(distinct.list());
    if multiplicities.len() != distinct.len() {
        return Ok(None);
    }
    let mut knots = Vec::new();
    for (multiplicity, knot) in multiplicities.iter().zip(distinct) {
        let count = geometry_or_none!(multiplicity
            .integer()
            .and_then(|count| usize::try_from(count).ok()));
        let knot = geometry_or_none!(knot.number().and_then(FiniteReal::new));
        if count == 0
            || knots
                .len()
                .checked_add(count)
                .is_none_or(|len| len > expected)
        {
            return Ok(None);
        }
        ctx.reserve_vec(&mut knots, count, "step_expanded_nurbs_knots")?;
        knots.extend(std::iter::repeat_with(|| knot.get()).take(count));
    }
    Ok(KnotVector::new(ctx, knots)?.ok())
}

fn references(value: &Value, ctx: &DecodeContext<'_>) -> Result<Option<Vec<u64>>, CodecError> {
    let values = geometry_or_none!(value.list());
    let mut references = Vec::new();
    for value in values {
        let id = geometry_or_none!(value.reference());
        ctx.push_vec(&mut references, id, "step_nurbs_control_point_ids")?;
    }
    Ok(Some(references))
}

pub(super) fn curve_carrier_record(id: u64, exchange: &Exchange) -> Option<u64> {
    let record = exchange.records().get(&id)?;
    if record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    }) {
        surface_curve_basis(record)
    } else {
        Some(id)
    }
}

fn numbers(value: &Value, ctx: &DecodeContext<'_>) -> Result<Option<Vec<f64>>, CodecError> {
    let values = geometry_or_none!(value.list());
    let mut numbers = Vec::new();
    for value in values {
        let number = geometry_or_none!(value.number());
        ctx.push_vec(&mut numbers, number, "step_nurbs_weight_values")?;
    }
    Ok(Some(numbers))
}

pub(super) fn normalize(vector: Vector3) -> Option<UnitVector3> {
    if !vector.is_finite() {
        return None;
    }
    let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
    if scale == 0.0 {
        return None;
    }
    let scaled = Vector3::new(vector.x / scale, vector.y / scale, vector.z / scale);
    UnitVector3::normalized_by_reciprocal(scaled)
}

fn project_axis(vector: UnitVector3, normal: UnitVector3) -> Option<UnitVector3> {
    let vector = *vector.recharted_by_reciprocal().as_raw();
    let normal = *normal.recharted_by_reciprocal().as_raw();
    normalize(vector - normal.scale(vector.dot(normal)))
}

fn second_project_axis(
    z_axis: UnitVector3,
    x_axis: UnitVector3,
    vector: UnitVector3,
) -> Option<UnitVector3> {
    let vector = *vector.recharted_by_reciprocal().as_raw();
    let z_axis = *z_axis.recharted_by_reciprocal().as_raw();
    let x_axis = *x_axis.recharted_by_reciprocal().as_raw();
    let projected = (vector - z_axis.scale(vector.dot(z_axis))) - x_axis.scale(vector.dot(x_axis));
    normalize(projected)
}

fn base_axis_3d(
    axis1: Option<UnitVector3>,
    axis2: Option<UnitVector3>,
    axis3: Option<UnitVector3>,
) -> Option<[UnitVector3; 3]> {
    let z_axis = axis3
        .unwrap_or(UnitVector3::Z_AXIS)
        .recharted_by_reciprocal();
    let default_x = default_reference_axis(z_axis);
    let x_axis = project_axis(axis1.unwrap_or(default_x), z_axis)?;
    let y_axis = second_project_axis(z_axis, x_axis, axis2.unwrap_or(UnitVector3::Y_AXIS))?;
    Some([x_axis, y_axis, z_axis])
}

const AXIS_PARALLEL_TOLERANCE: f64 = EPS_GEOMETRY_READ_EXACT_GEOMETRY;

fn default_reference_axis(axis: UnitVector3) -> UnitVector3 {
    if axis.as_raw().x.abs() >= 1.0 - AXIS_PARALLEL_TOLERANCE {
        UnitVector3::Y_AXIS
    } else {
        UnitVector3::X_AXIS
    }
}

fn transformation_direction<T: Copy>(
    record: &RawRecord,
    name: &str,
    index: usize,
    directions: &BTreeMap<u64, T>,
) -> Result<Option<T>, ()> {
    match transformation_parameter(record, name, index).ok_or(())? {
        Value::Omitted | Value::Derived => Ok(None),
        Value::Reference(id) => directions.get(id).copied().map(Some).ok_or(()),
        _ => Err(()),
    }
}

fn cartesian_transformation_operator(
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    directions: &BTreeMap<u64, UnitVector3>,
) -> Option<Transform> {
    let axis1 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        0,
        directions,
    )
    .ok()?;
    let axis2 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        1,
        directions,
    )
    .ok()?;
    let origin = transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 2)?
        .reference()
        .and_then(|id| points.get(&id).copied().map(FinitePoint3::get))?;
    let scale = match transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 3) {
        Some(Value::Omitted | Value::Derived) | None => 1.0,
        Some(value) => value.number()?,
    };
    let scale = PositiveReal::new(scale)?;
    let axis3 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        4,
        directions,
    )
    .ok()?;
    let [axis_x, axis_y, axis_z] = base_axis_3d(axis1, axis2, axis3)?;
    let axis_x = axis_x.as_raw();
    let axis_y = axis_y.as_raw();
    let axis_z = axis_z.as_raw();
    Transform::affine([
        [
            axis_x.x * scale.get(),
            axis_y.x * scale.get(),
            axis_z.x * scale.get(),
            origin.x,
        ],
        [
            axis_x.y * scale.get(),
            axis_y.y * scale.get(),
            axis_z.y * scale.get(),
            origin.y,
        ],
        [
            axis_x.z * scale.get(),
            axis_y.z * scale.get(),
            axis_z.z * scale.get(),
            origin.z,
        ],
    ])
}

fn cartesian_transformation_operator_2d(
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    directions: &BTreeMap<u64, HypotDirection2>,
) -> Option<Transform2> {
    let axis1 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        0,
        directions,
    )
    .ok()?;
    let axis2 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        1,
        directions,
    )
    .ok()?;
    let (axis1, axis2) = base_axis_2d(axis1, axis2);
    let origin = transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 2)?
        .reference()
        .and_then(|id| points.get(&id).copied())?;
    let scale = match transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 3) {
        Some(Value::Omitted | Value::Derived) | None => 1.0,
        Some(value) => value.number()?,
    };
    let scale = PositiveReal::new(scale)?;
    let [axis1_u, axis1_v] = axis1.get();
    let [axis2_u, axis2_v] = axis2.get();
    Transform2::affine([
        [axis1_u * scale.get(), axis2_u * scale.get(), origin.u],
        [axis1_v * scale.get(), axis2_v * scale.get(), origin.v],
    ])
}

fn base_axis_2d(
    axis1: Option<HypotDirection2>,
    axis2: Option<HypotDirection2>,
) -> (HypotDirection2, HypotDirection2) {
    match (axis1, axis2) {
        (Some(axis1), axis2) => {
            let axis1 = axis1.recharted_by_hypot();
            let mut perpendicular = axis1.quarter_turn();
            if let Some(axis2) = axis2 {
                let [axis2_u, axis2_v] = axis2.recharted_by_hypot().get();
                let [perpendicular_u, perpendicular_v] = perpendicular.get();
                if axis2_u * perpendicular_u + axis2_v * perpendicular_v < 0.0 {
                    perpendicular = axis1.reverse_quarter_turn();
                }
            }
            (axis1, perpendicular)
        }
        (None, Some(axis2)) => {
            let axis2 = axis2.recharted_by_hypot();
            (axis2.reverse_quarter_turn(), axis2)
        }
        (None, None) => (HypotDirection2::X_AXIS, HypotDirection2::Y_AXIS),
    }
}

fn optional_direction(
    value: Option<&Value>,
    directions: &BTreeMap<u64, UnitVector3>,
) -> Option<UnitVector3> {
    match value? {
        Value::Omitted => None,
        Value::Reference(id) => directions.get(id).copied(),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
