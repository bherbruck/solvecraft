// SPDX-License-Identifier: Apache-2.0
//! Converts IR geometry carriers into STEP DATA instances.
//!
//! Conversion appends supporting points, directions, and placements before
//! returning the top-level carrier reference. Analytic carriers use their STEP
//! counterparts. NURBS carriers use `*_WITH_KNOTS`, with complex instances for
//! rational geometry.

use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::PcurveGeometry,
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::{DirectionAboveEpsilon, UnitVector3};

use crate::writer::{refs, Emitter, Ref};

const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12: f64 = 1.0e-12;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12: f64 = 1.0e-12;

/// The affine placements over `surface`, outermost first, and the basis carrier
/// under them.
///
/// `cadmpeg_ir::geometry::PlacedSurface::try_new` refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING), so
/// the walk reads at most that many placements. It is iterative, so it costs
/// no stack.
fn placed_surface(surface: &SolvedSurfaceGeometry) -> (Vec<&Transform>, &SolvedSurfaceGeometry) {
    let mut placements = Vec::new();
    let mut geometry = surface;
    while let SolvedSurfaceGeometry::Transformed(placed) = geometry {
        placements.push(placed.transform());
        geometry = placed.basis();
    }
    (placements, geometry)
}

pub(crate) fn surface_is_supported(surface: &SolvedSurfaceGeometry) -> bool {
    let (placements, basis) = placed_surface(surface);
    placements
        .iter()
        .all(|transform| similarity_transform(transform).is_some())
        && match basis {
            SolvedSurfaceGeometry::Plane(_)
            | SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Sphere(_)
            | SolvedSurfaceGeometry::Torus(_) => true,
            SolvedSurfaceGeometry::Nurbs(n) => valid_nurbs_surface(n),
            // `placed_surface` ends the walk at the first carrier that is not a
            // placement, so the basis is never `Transformed`.
            SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Unknown { .. } => false,
        }
}

fn valid_nurbs_surface(n: &NurbsSurface) -> bool {
    n.pole_weights()
        .is_none_or(|weights| weights.iter().all(|weight| weight.get() > 0.0))
}

pub(crate) fn curve_is_supported(curve: &CurveGeometry) -> bool {
    // A composite carrier is emitted from its child graph by the exporter, so it
    // is supported only in the outermost position.
    matches!(
        curve,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    ) || curve.solved().is_some_and(leaf_curve_is_supported)
}

/// The affine placements over `curve`, outermost first, and the basis carrier
/// under them.
///
/// The walk is iterative and bounded exactly as [`placed_surface`] is:
/// [`leaf_curve_is_supported`] and [`curve`] take their bound here.
fn placed_curve(curve: &SolvedCurveGeometry) -> (Vec<&Transform>, &SolvedCurveGeometry) {
    let mut placements = Vec::new();
    let mut geometry = curve;
    while let SolvedCurveGeometry::Transformed(placed) = geometry {
        placements.push(placed.transform());
        geometry = placed.basis();
    }
    (placements, geometry)
}

fn leaf_curve_is_supported(curve: &SolvedCurveGeometry) -> bool {
    let (placements, basis) = placed_curve(curve);
    placements
        .iter()
        .all(|transform| similarity_transform(transform).is_some())
        && match basis {
            SolvedCurveGeometry::Line(_)
            | SolvedCurveGeometry::Circle(_)
            | SolvedCurveGeometry::Ellipse(_)
            | SolvedCurveGeometry::Parabola(_)
            | SolvedCurveGeometry::Hyperbola(_)
            | SolvedCurveGeometry::Degenerate(_)
            | SolvedCurveGeometry::Nurbs(_)
            | SolvedCurveGeometry::Polyline(_) => true,
            // `placed_curve` ends the walk at the first carrier that is not a
            // placement, so the basis is never `Transformed`.
            SolvedCurveGeometry::Transformed(_)
            | SolvedCurveGeometry::Composite { .. }
            | SolvedCurveGeometry::Unknown { .. } => false,
        }
}

struct SimilarityColumns {
    directions: [DirectionAboveEpsilon; 3],
    scale: f64,
}

fn similarity_transform(transform: &Transform) -> Option<SimilarityColumns> {
    let columns = [
        Vector3::new(
            transform.rows()[0][0],
            transform.rows()[1][0],
            transform.rows()[2][0],
        ),
        Vector3::new(
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1],
        ),
        Vector3::new(
            transform.rows()[0][2],
            transform.rows()[1][2],
            transform.rows()[2][2],
        ),
    ];
    let [Some(x), Some(y), Some(z)] = columns.map(DirectionAboveEpsilon::new) else {
        return None;
    };
    let directions = [x, y, z];
    let scale = x.norm();
    if !scale.is_finite() || scale <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12 {
        return None;
    }
    let columns = directions.map(|direction| {
        let v = direction.get();
        Vector3::new(v.x / scale, v.y / scale, v.z / scale)
    });
    (columns
        .iter()
        .all(|v| (v.norm() - 1.0).abs() <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10)
        && columns[0].dot(columns[1]).abs() <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10
        && columns[0].dot(columns[2]).abs() <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10
        && columns[1].dot(columns[2]).abs() <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10)
        .then_some(SimilarityColumns { directions, scale })
}

/// Emit or reuse a `CARTESIAN_POINT`.
pub(crate) fn point(e: &mut Emitter, p: Point3) -> Ref {
    let params = format!("'',({},{},{})", e.real(p.x), e.real(p.y), e.real(p.z));
    e.emit_interned("CARTESIAN_POINT", &params)
}

fn point2(e: &mut Emitter, p: Point2) -> Ref {
    let params = format!("'',({},{})", e.real(p.u), e.real(p.v));
    e.emit_interned("CARTESIAN_POINT", &params)
}

fn direction2(e: &mut Emitter, v: Point2) -> Option<Ref> {
    let unit =
        cadmpeg_ir::features::FiniteVector3::new(Vector3::new(v.u, v.v, 0.0))?.unit_nonzero()?;
    Some(e.emit_interned(
        "DIRECTION",
        &format!("'',({},{})", e.real(unit.x), e.real(unit.y)),
    ))
}

fn similarity_transform_2d(transform: &Transform2) -> bool {
    let first = Point2::new(transform.rows()[0][0], transform.rows()[1][0]);
    let second = Point2::new(transform.rows()[0][1], transform.rows()[1][1]);
    let scale = first.u.hypot(first.v);
    if !scale.is_finite() || scale <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12 {
        return false;
    }
    let first = Point2::new(first.u / scale, first.v / scale);
    let second = Point2::new(second.u / scale, second.v / scale);
    (second.u.hypot(second.v) - 1.0).abs() <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10
        && (first.u * second.u + first.v * second.v).abs()
            <= EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10
}

fn axis2_placement_2d(e: &mut Emitter, location: Point2, x_axis: Point2) -> Option<Ref> {
    let location = point2(e, location);
    let direction = direction2(e, x_axis)?;
    Some(e.emit("AXIS2_PLACEMENT_2D", &format!("'',{location},{direction}")))
}

fn transformation_operator_2d(e: &mut Emitter, transform: Transform2) -> Option<Ref> {
    let origin = point2(
        e,
        Point2::new(transform.rows()[0][2], transform.rows()[1][2]),
    );
    let x = Point2::new(transform.rows()[0][0], transform.rows()[1][0]);
    let y = Point2::new(transform.rows()[0][1], transform.rows()[1][1]);
    let scale = x.u.hypot(x.v);
    let x = direction2(e, x)?;
    let y = direction2(e, y)?;
    if !scale.is_finite() {
        return None;
    }
    Some(e.emit(
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        &format!("'',{x},{y},{origin},{}", e.real(scale)),
    ))
}

/// Emit a two-dimensional curve for use inside a `PCURVE` representation.
///
/// `Transformed`, `Trimmed` and `Offset` each hold their basis inline in a
/// `Box`, and each constructor refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING)
/// carriers over one leaf, so the recursion here is bounded by the carrier.
/// Each nesting arm emits its own record after the basis record it references,
/// so the chain is written from the leaf outwards.
pub(crate) fn pcurve(e: &mut Emitter, geometry: &PcurveGeometry) -> Option<Ref> {
    Some(match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            let point = point2(e, *origin);
            let magnitude = direction.u.hypot(direction.v);
            if !magnitude.is_finite() {
                return None;
            }
            let direction = direction2(e, *direction)?;
            let vector = e.emit("VECTOR", &format!("'',{direction},{}", e.real(magnitude)));
            e.emit("LINE", &format!("'',{point},{vector}"))
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let radius = circle_pcurve.radius();
            let placement = axis2_placement_2d(e, center.get(), x_axis.get())?;
            e.emit(
                "CIRCLE",
                &format!("'',{placement},{}", e.real(radius.get())),
            )
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            let placement = axis2_placement_2d(e, center.get(), x_axis.get())?;
            e.emit(
                "ELLIPSE",
                &format!(
                    "'',{placement},{},{}",
                    e.real(major_radius.get()),
                    e.real(minor_radius.get())
                ),
            )
        }
        PcurveGeometry::Parabola(parabola_pcurve) => {
            let vertex = parabola_pcurve.vertex();
            let x_axis = parabola_pcurve.x_axis();
            let focal_distance = parabola_pcurve.focal_distance();
            let placement = axis2_placement_2d(e, vertex.get(), x_axis.get())?;
            e.emit(
                "PARABOLA",
                &format!("'',{placement},{}", e.real(focal_distance.get())),
            )
        }
        PcurveGeometry::Hyperbola(hyperbola_pcurve) => {
            let center = hyperbola_pcurve.center();
            let x_axis = hyperbola_pcurve.x_axis();
            let major_radius = hyperbola_pcurve.major_radius();
            let minor_radius = hyperbola_pcurve.minor_radius();
            let placement = axis2_placement_2d(e, center.get(), x_axis.get())?;
            e.emit(
                "HYPERBOLA",
                &format!(
                    "'',{placement},{},{}",
                    e.real(major_radius.get()),
                    e.real(minor_radius.get())
                ),
            )
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let points = nurbs
                .control_points()
                .iter()
                .map(|point| point2(e, point.get()))
                .collect::<Vec<_>>();
            let (knots, multiplicities) = compress_knots(nurbs.knots());
            let base = format!(
                "{},{},.UNSPECIFIED.,{},.U.",
                nurbs.degree(),
                refs(&points),
                closed_flag(nurbs.periodic())
            );
            let with_knots = format!(
                "{},{},.UNSPECIFIED.",
                int_list(&multiplicities),
                real_list(e, &knots)
            );
            if let Some(weights) = nurbs.pole_rows().weights() {
                e.emit_raw(
                    "B_SPLINE_CURVE_WITH_KNOTS",
                    &format!(
                        "( BOUNDED_CURVE() B_SPLINE_CURVE({base}) B_SPLINE_CURVE_WITH_KNOTS({with_knots}) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE({}) REPRESENTATION_ITEM('') )",
                        real_list(e, &weights)
                    ),
                )
            } else {
                e.emit(
                    "B_SPLINE_CURVE_WITH_KNOTS",
                    &format!("'',{base},{with_knots}"),
                )
            }
        }
        PcurveGeometry::Transformed(placed) => {
            let transform = placed.transform();
            if !similarity_transform_2d(transform) {
                return None;
            }
            let basis = pcurve(e, placed.basis())?;
            let operator = transformation_operator_2d(e, *transform)?;
            e.emit("CURVE_REPLICA", &format!("'',{basis},{operator}"))
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let same_sense = trimmed_pcurve.same_sense();
            let basis = trimmed_pcurve.basis();
            let basis = pcurve(e, basis)?;
            let sense = if same_sense { ".T." } else { ".F." };
            e.emit(
                "TRIMMED_CURVE",
                &format!(
                    "'',{basis},({}),({}),{sense},.PARAMETER.",
                    e.real(parameter_range.endpoints()[0]),
                    e.real(parameter_range.endpoints()[1])
                ),
            )
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let basis = pcurve(e, basis)?;
            e.emit(
                "OFFSET_CURVE_2D",
                &format!("'',{basis},{},.F.", e.real(distance.get())),
            )
        }
        PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle(_) => return None,
    })
}

/// Emit a pcurve in the reversed parameter chart of a negative-angle cone.
/// The half-turn maps each `(u, v)` to `(-u, -v)` without changing the curve's
/// own parameter or sense.
pub(crate) fn pcurve_on_reversed_cone(e: &mut Emitter, geometry: &PcurveGeometry) -> Option<Ref> {
    let basis = pcurve(e, geometry)?;
    let reversal = Transform2::affine([[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]])?;
    let operator = transformation_operator_2d(e, reversal)?;
    Some(e.emit("CURVE_REPLICA", &format!("'',{basis},{operator}")))
}

/// Emit or reuse a unit-length `DIRECTION` from an admitted nonzero vector.
pub(crate) fn direction(e: &mut Emitter, v: DirectionAboveEpsilon) -> Ref {
    let normalized = v.normalized();
    let u = normalized.as_raw();
    let params = format!("'',({},{},{})", e.real(u.x), e.real(u.y), e.real(u.z));
    e.emit_interned("DIRECTION", &params)
}

/// Emit an `AXIS2_PLACEMENT_3D` with the given origin, local +Z axis, and local
/// +X reference direction.
///
/// STEP projects the reference direction onto the plane normal to the axis.
pub(crate) fn placement(
    e: &mut Emitter,
    origin: Point3,
    axis: UnitVector3,
    ref_dir: UnitVector3,
) -> Ref {
    let o = point(e, origin);
    let a = direction(e, axis.into());
    let r = direction(e, ref_dir.into());
    e.emit("AXIS2_PLACEMENT_3D", &format!("'',{o},{a},{r}"))
}

/// Emit a `CARTESIAN_TRANSFORMATION_OPERATOR_3D`, which states three axis
/// directions, an origin and one scale: a similarity. Any other transform,
/// including one with a zero column, is refused through
/// [`Emitter::refuse_operator`], and the section is not written.
pub(crate) fn transformation_operator(e: &mut Emitter, transform: Transform) -> Ref {
    let Some(SimilarityColumns {
        directions: [x, y, z],
        scale,
    }) = similarity_transform(&transform)
    else {
        e.refuse_operator(transform);
        // The emitter refuses the whole DATA section, so no operator reference is written.
        return Ref(0);
    };
    let origin = point(
        e,
        Point3::new(
            transform.rows()[0][3],
            transform.rows()[1][3],
            transform.rows()[2][3],
        ),
    );
    let x = direction(e, x);
    let y = direction(e, y);
    let z = direction(e, z);
    e.emit(
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        &format!("'',{x},{y},{origin},{},{z}", e.real(scale)),
    )
}

/// The carrier whose values [`surface`] writes into the file for `g`.
///
/// [`surface`] emits a `Transformed` carrier as a `SURFACE_REPLICA` over the
/// record of its basis, so the radii and angles in the file are the basis's.
/// The export census reads them here.
pub(crate) fn emitted_basis(g: &SolvedSurfaceGeometry) -> &SolvedSurfaceGeometry {
    let (_, basis) = placed_surface(g);
    basis
}

/// Emit an analytic or NURBS surface carrier.
pub(crate) fn surface(e: &mut Emitter, g: &SolvedSurfaceGeometry) -> Option<Ref> {
    let (placements, basis) = placed_surface(g);
    let mut reference = basis_surface(e, basis)?;
    // The chain is emitted from the basis outwards, so each `SURFACE_REPLICA`
    // references the record written for the placement inside it.
    for transform in placements.iter().rev() {
        let operator = transformation_operator(e, **transform);
        reference = e.emit("SURFACE_REPLICA", &format!("'',{reference},{operator}"));
    }
    Some(reference)
}

/// Emit the basis carrier under a chain of affine placements.
fn basis_surface(e: &mut Emitter, g: &SolvedSurfaceGeometry) -> Option<Ref> {
    Some(match g {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = *plane_surface.frame().axis();
            let u_axis = *plane_surface.frame().reference();
            let pl = placement(e, origin, normal, u_axis);
            e.emit("PLANE", &format!("'',{pl}"))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis();
            let ref_direction = *cylinder_surface.frame().reference();
            let radius = cylinder_surface.radius().get();
            let pl = placement(e, origin, axis, ref_direction);
            e.emit(
                "CYLINDRICAL_SURFACE",
                &format!("'',{pl},{}", e.real(radius)),
            )
        }
        // ISO 10303-42 `conical_surface` holds `semi_angle` in `(0, pi/2)`.
        // Reversing the axis and angle maps a negative-angle cone exactly under
        // the parameter change `(u, v) -> (-u, -v)`.
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = *cone_surface.frame().axis();
            let ref_direction = *cone_surface.frame().reference();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            let written_axis = if half_angle < 0.0 {
                axis.reversed()
            } else {
                axis
            };
            let pl = placement(e, origin, written_axis, ref_direction);
            if half_angle.abs() <= 1e-9 {
                e.emit(
                    "CYLINDRICAL_SURFACE",
                    &format!("'',{pl},{}", e.real(radius.abs())),
                )
            } else {
                e.emit(
                    "CONICAL_SURFACE",
                    &format!("'',{pl},{},{}", e.real(radius), e.real(half_angle.abs())),
                )
            }
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = *sphere_surface.frame().axis();
            let ref_direction = *sphere_surface.frame().reference();
            let radius = sphere_surface.radius().get();
            let pl = placement(e, center, axis, ref_direction);
            e.emit(
                "SPHERICAL_SURFACE",
                &format!("'',{pl},{}", e.real(radius.abs())),
            )
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = *torus_surface.frame().axis();
            let ref_direction = *torus_surface.frame().reference();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let pl = placement(e, center, axis, ref_direction);
            e.emit(
                "TOROIDAL_SURFACE",
                &format!(
                    "'',{pl},{},{}",
                    e.real(major_radius),
                    e.real(minor_radius.abs())
                ),
            )
        }
        SolvedSurfaceGeometry::Nurbs(n) => nurbs_surface(e, n)?,
        // These carrier families have no direct STEP representation; callers
        // report the omitted carrier instead of fabricating a placeholder.
        // `placed_surface` ends the walk at the first carrier that is not a
        // placement, so `Transformed` does not reach this function.
        SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => return None,
    })
}

/// Emit an analytic or NURBS 3D curve carrier.
pub(crate) fn curve(e: &mut Emitter, g: &SolvedCurveGeometry) -> Option<Ref> {
    let (placements, basis) = placed_curve(g);
    let mut reference = basis_curve(e, basis)?;
    // The chain is emitted from the basis outwards, so each `CURVE_REPLICA`
    // references the record written for the placement inside it.
    for transform in placements.iter().rev() {
        let operator = transformation_operator(e, **transform);
        reference = e.emit("CURVE_REPLICA", &format!("'',{reference},{operator}"));
    }
    Some(reference)
}

/// Emit the basis carrier under a chain of affine placements.
fn basis_curve(e: &mut Emitter, g: &SolvedCurveGeometry) -> Option<Ref> {
    Some(match g {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let d = line_curve.direction();
            let p = point(e, origin);
            // A LINE's VECTOR carries the direction; unit magnitude is conventional.
            let dir = direction(e, d.into());
            let vec = e.emit("VECTOR", &format!("'',{dir},{}", e.real(1.0)));
            e.emit("LINE", &format!("'',{p},{vec}"))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = *circle_curve.frame().axis();
            let ref_direction = *circle_curve.frame().reference();
            let radius = circle_curve.radius().get();
            let pl = placement(e, center, axis, ref_direction);
            e.emit("CIRCLE", &format!("'',{pl},{}", e.real(radius)))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = *ellipse_curve.frame().axis();
            let major_direction = *ellipse_curve.frame().reference();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let pl = placement(e, center, axis, major_direction);
            e.emit(
                "ELLIPSE",
                &format!("'',{pl},{},{}", e.real(major_radius), e.real(minor_radius)),
            )
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = *parabola_curve.frame().axis();
            let major_direction = *parabola_curve.frame().reference();
            let focal_distance = parabola_curve.focal_distance().get();
            let pl = placement(e, vertex, axis, major_direction);
            e.emit("PARABOLA", &format!("'',{pl},{}", e.real(focal_distance)))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center().get();
            let axis = *hyperbola_curve.frame().axis();
            let major_direction = *hyperbola_curve.frame().reference();
            let major_radius = hyperbola_curve.major_radius().get();
            let minor_radius = hyperbola_curve.minor_radius().get();
            let pl = placement(e, center, axis, major_direction);
            e.emit(
                "HYPERBOLA",
                &format!("'',{pl},{},{}", e.real(major_radius), e.real(minor_radius)),
            )
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => {
            let collapsed = degenerate_curve.point().get();
            let point = point(e, collapsed);
            e.emit("POLYLINE", &format!("'',({point},{point})"))
        }
        SolvedCurveGeometry::Nurbs(n) => nurbs_curve(e, n),
        SolvedCurveGeometry::Polyline(polyline) => {
            let points = polyline
                .points()
                .map(|position| point(e, position.get()).to_string())
                .collect::<Vec<_>>()
                .join(",");
            e.emit("POLYLINE", &format!("'',({points})"))
        }
        // `placed_curve` ends the walk at the first carrier that is not a
        // placement, so `Transformed` does not reach this function. A composite
        // carrier is emitted from its child graph by the exporter, and an
        // unknown carrier has no STEP record.
        SolvedCurveGeometry::Transformed(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => return None,
    })
}

/// Convert a repeated knot vector into ordered values and multiplicities.
fn compress_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &k in knots {
        match runs.last_mut() {
            Some((value, multiplicity)) if *value == k => *multiplicity += 1,
            _ => runs.push((k, 1)),
        }
    }
    runs.into_iter().unzip()
}

fn int_list(xs: &[usize]) -> String {
    let mut out = String::from("(");
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&x.to_string());
    }
    out.push(')');
    out
}

fn real_list(e: &Emitter, xs: &[f64]) -> String {
    let mut out = String::from("(");
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&e.real(*x));
    }
    out.push(')');
    out
}

fn closed_flag(periodic: bool) -> &'static str {
    if periodic {
        ".T."
    } else {
        ".F."
    }
}

fn nurbs_curve(e: &mut Emitter, n: &NurbsCurve) -> Ref {
    let pts: Vec<Ref> = n
        .control_points()
        .iter()
        .map(|p| point(e, p.get()))
        .collect();
    let (knots, mults) = compress_knots(n.knots());
    let ctrl = refs(&pts);
    let base = format!(
        "{},{ctrl},.UNSPECIFIED.,{},.U.",
        n.degree(),
        closed_flag(n.periodic())
    );
    let with_knots = format!(
        "{},{},.UNSPECIFIED.",
        int_list(&mults),
        real_list(e, &knots)
    );
    match n.pole_rows().weights() {
        None => e.emit(
            "B_SPLINE_CURVE_WITH_KNOTS",
            &format!("'',{base},{with_knots}"),
        ),
        Some(w) => {
            // Rational curves require the AND-combined complex instance form.
            let body = format!(
                "( BOUNDED_CURVE() B_SPLINE_CURVE({base}) \
                 B_SPLINE_CURVE_WITH_KNOTS({with_knots}) CURVE() \
                 GEOMETRIC_REPRESENTATION_ITEM() \
                 RATIONAL_B_SPLINE_CURVE({}) REPRESENTATION_ITEM('') )",
                real_list(e, &w)
            );
            e.emit_raw("B_SPLINE_CURVE_WITH_KNOTS", &body)
        }
    }
}

fn nurbs_surface(e: &mut Emitter, n: &NurbsSurface) -> Option<Ref> {
    if !valid_nurbs_surface(n) {
        return None;
    }
    // IR control points are u-major: index i*v_count + j is pole (i, j). STEP's
    // control_points_list is LIST(u) OF LIST(v), so the outer list runs over u.
    let mut rows: Vec<String> = Vec::new();
    for grid_row in n.control_grid() {
        let mut row: Vec<Ref> = Vec::new();
        for p in grid_row {
            row.push(point(e, p.get()));
        }
        rows.push(refs(&row));
    }
    let grid = format!("({})", rows.join(","));

    let (u_knots, u_mults) = compress_knots(n.u_knots());
    let (v_knots, v_mults) = compress_knots(n.v_knots());
    let base = format!(
        "{},{},{grid},.UNSPECIFIED.,{},{},.U.",
        n.u_degree(),
        n.v_degree(),
        closed_flag(n.u_periodic()),
        closed_flag(n.v_periodic())
    );
    let with_knots = format!(
        "{},{},{},{},.UNSPECIFIED.",
        int_list(&u_mults),
        int_list(&v_mults),
        real_list(e, &u_knots),
        real_list(e, &v_knots)
    );
    Some(match n.pole_grid().weights() {
        None => e.emit(
            "B_SPLINE_SURFACE_WITH_KNOTS",
            &format!("'',{base},{with_knots}"),
        ),
        Some(w) => {
            // Rational surface weights are LIST(u) OF LIST(v), matching the grid.
            let mut wrows: Vec<String> = Vec::new();
            for row in w {
                wrows.push(real_list(e, &row));
            }
            let wgrid = format!("({})", wrows.join(","));
            let body = format!(
                "( BOUNDED_SURFACE() B_SPLINE_SURFACE({base}) \
                 B_SPLINE_SURFACE_WITH_KNOTS({with_knots}) \
                 GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE({wgrid}) \
                 REPRESENTATION_ITEM('') SURFACE() )"
            );
            e.emit_raw("B_SPLINE_SURFACE_WITH_KNOTS", &body)
        }
    })
}

#[cfg(test)]
mod support_tests;
