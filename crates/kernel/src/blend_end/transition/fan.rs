//! Join an endpoint fan with common boundary splines and tangent planes.
//! Isolated singular tips accommodate the old faces' different limiting normals.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    faces: &[&mt::Face],
    fan: &[(mt::Edge, usize)],
    points: &[mt::Vertex],
    wire: &[mt::Edge],
    boundaries: &[Boundary],
    section: &mt::Edge,
    center: Vec3,
    axis: Vec3,
    sign: f64,
    size: f64,
    planes: &[(Vec3, Vec3)],
    out: Vec3,
    singular_fan: bool,
) -> Result<(Vec<mt::Edge>, Vec<mt::Face>)> {
    let curve = section.curve();
    let (lo, hi) = curve.range_tuple();
    let mut parameters = vec![lo];
    let mut arc = vec![section.front().clone()];
    for p in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        let u = curve.search_nearest_parameter(p.point(), None, 100).ok_or_else(|| unsupported("transition fan angular parameter"))?;
        if !u.is_finite() || u <= parameters.last().copied().unwrap_or(lo) + 1e-5 || u >= hi - 1e-5 {
            return Err(unsupported("transition fan folds across the cylinder"));
        }
        parameters.push(u);
        arc.push(builder::vertex(curve.subs(u)));
    }
    parameters.push(hi);
    arc.push(section.back().clone());
    let mut seams = vec![wire.get(1).cloned().ok_or_else(|| unsupported("transition wall rail"))?];
    for (i, point) in points.iter().enumerate().skip(1).take(points.len().saturating_sub(2)) {
        let a = arc.get(i).ok_or_else(|| unsupported("transition fan vertex"))?;
        let (edge, _) = fan.get(i).ok_or_else(|| unsupported("transition original seam"))?;
        let curve = edge.oriented_curve();
        let (_, t) = curve.range_tuple();
        let d = curve.der(t);
        let d = Vec3::new(d.x, d.y, d.z);
        let speed = d.len();
        let length = vtx(a).dist(vtx(point));
        if !speed.is_finite() || speed < 1e-10 || !length.is_finite() || length < size * 1e-8 {
            return Err(unsupported("degenerate transition seam"));
        }
        let start = vtx(a);
        let end = vtx(point);
        let ds = -axis * length;
        // Use the actual adjacent surface normals, not the derivative of an
        // approximated intersection spline, for the crease's terminal frame.
        let normal = |index: usize| -> Result<Vec3> {
            let (_, fi) = fan.get(index).ok_or_else(|| unsupported("transition seam face"))?;
            let face = faces.get(*fi).ok_or_else(|| unsupported("transition seam face"))?;
            let surface = face.oriented_surface();
            let (u, v) = surface.search_nearest_parameter(point.point(), None, 100).ok_or_else(|| unsupported("transition seam parameters"))?;
            let n = surface.normal(u, v);
            Ok(Vec3::new(n.x, n.y, n.z))
        };
        let na = normal(i - 1)?;
        let nb = normal(i)?;
        let common = (na + nb).normalized().ok_or_else(|| unsupported("opposite terminal normals"))?;
        let chord = end - start;
        let mut tangent = (chord - common * chord.dot(common)).normalized().ok_or_else(|| unsupported("terminal direction"))?;
        if tangent.dot(end - start) < 0.0 {
            tangent = -tangent;
        }
        let mut end_length = length;
        let mut controls = [start; 6];
        let mut bounded = false;
        for _ in 0..16 {
            let de = tangent * end_length;
            controls = [start, start + ds / 5.0, start + ds * 0.4, end - de * 0.4, end - de / 5.0, end];
            if controls.iter().all(|p| p.is_finite() && planes.iter().all(|(n, q)| (*p - *q).dot(*n) <= size * 1e-8)) {
                bounded = true;
                break;
            }
            end_length *= 0.5;
        }
        if !bounded || end_length <= size * 1e-9 {
            return Err(unsupported("transition seam cannot stay within its local bounds"));
        }
        let curve = mt::Curve::BSplineCurve(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(5), controls.into_iter().map(p3).collect()));
        seams.push(mt::Edge::try_new(a, point, curve).map_err(|e| KernelError::Failed(format!("transition shared seam: {e}")))?);
    }
    seams.push(wire.last().cloned().ok_or_else(|| unsupported("transition wall rail"))?.inverse());
    let cylinder = if sign > 0.0 { Normal::InwardCylinder { origin: center, axis } } else { Normal::Cylinder { origin: center, axis } };
    let mut sections = Vec::new();
    let mut patches = Vec::new();
    let mut pieces = Vec::new();
    for (i, span) in parameters.windows(2).enumerate() {
        let [lo, hi] = span else {
            return Err(unsupported("transition section span"));
        };
        let (Some(a), Some(b), Some(sa), Some(sb)) = (arc.get(i), arc.get(i + 1), seams.get(i), seams.get(i + 1)) else {
            return Err(unsupported("transition fan section"));
        };
        let edge = mt::Edge::try_new(a, b, trim(curve.clone(), *lo, *hi)).map_err(|e| KernelError::Failed(format!("transition arc: {e}")))?;
        let end = wire.get(i + 2).ok_or_else(|| unsupported("transition end boundary"))?;
        let end_normal = boundaries.get(i + 2).ok_or_else(|| unsupported("transition end normal"))?.normal.clone();

        let seam_normal = |seam: &mt::Edge, index: usize, reverse: bool| -> Result<Normal> {
            if index == 0 {
                return Ok(boundaries.get(1).ok_or_else(|| unsupported("transition rail normal"))?.normal.clone());
            }
            if index + 1 == points.len() {
                return Ok(boundaries.last().ok_or_else(|| unsupported("transition rail normal"))?.normal.clone());
            }
            let c = seam.oriented_curve();
            let (a, _b) = c.range_tuple();
            let boundary = Boundary { curve: c.clone(), normal: cylinder.clone() };
            let n0 = boundary.sample(a)?.1;
            let terminal_normal = |which: usize| -> Result<Vec3> {
                let (_, fi) = fan.get(which).ok_or_else(|| unsupported("fan face"))?;
                let face = faces.get(*fi).ok_or_else(|| unsupported("fan face"))?;
                let surface = face.oriented_surface();
                let (u, v) = surface.search_nearest_parameter(seam.back().point(), None, 100).ok_or_else(|| unsupported("fan terminal normal"))?;
                let n = surface.normal(u, v);
                Ok(Vec3::new(n.x, n.y, n.z))
            };
            let mut n1 =
                (terminal_normal(index - 1)? + terminal_normal(index)?).normalized().ok_or_else(|| unsupported("opposite terminal normals"))?;
            if n1.dot(out) < 0.0 {
                n1 = -n1;
            }
            Ok(if reverse {
                Normal::Hermite { start: n1, end: n0, start_der: Vec3::ZERO, end_der: Vec3::ZERO }
            } else {
                Normal::Hermite { start: n0, end: n1, start_der: Vec3::ZERO, end_der: Vec3::ZERO }
            })
        };
        let conditions = [
            Boundary { curve: edge.oriented_curve().inverse(), normal: cylinder.clone() },
            Boundary { curve: sa.oriented_curve(), normal: seam_normal(sa, i, false)? },
            Boundary { curve: end.oriented_curve(), normal: end_normal },
            Boundary { curve: sb.oriented_curve().inverse(), normal: seam_normal(sb, i + 1, true)? },
        ];
        let terminal = |boundary: &Boundary, end: bool| -> Result<Vec3> {
            let (lo, hi) = boundary.curve.range_tuple();
            Ok(boundary.sample(if end { hi } else { lo })?.1)
        };
        // Collapse only a genuinely creased tip. Applying the singular map at
        // a smooth meeting introduces needless stationary modes and wrinkles.
        let singular = [
            i > 0 && terminal(&conditions[1], true)?.dot(terminal(&conditions[2], false)?) < 1.0 - 1e-6,
            i + 2 < points.len() && terminal(&conditions[3], false)?.dot(terminal(&conditions[2], true)?) < 1.0 - 1e-6,
        ];
        pieces.push((vec![edge.inverse(), sa.clone(), end.clone(), sb.inverse()], conditions, singular));
        sections.push(edge);
    }
    crate::blend_corner::setback::partition::check(&pieces.iter().map(|(wire, _, _)| wire.clone()).collect::<Vec<_>>(), out, size)?;
    let mut fitted = Vec::new();
    for density in [20, 32, 40, 56] {
        // Each sector has its own fit and certificate. Fit them concurrently,
        // retaining their boundary order, before validating and sewing the seams.
        let attempt = crate::par::map("fillet fan sector", &pieces, true, |(wire, conditions, singular)| {
            crate::blend_corner::setback::parametric::fit(conditions, *singular, out, size, planes, density).map(|surface| (wire.clone(), surface))
        })
        .into_iter()
        .collect::<Result<Vec<_>>>();
        if let Ok(value) = attempt {
            fitted = value;
            break;
        }
        // A smooth fan can use a single trimmed graph when its rectangular
        // chart does not fit. Avoid repeatedly refining an unsuitable chart.
        if !singular_fan {
            break;
        }
    }
    if fitted.is_empty() {
        return Err(unsupported("cannot fit a common fan boundary grid"));
    }
    let boundary_curve = |surface: &mt::Surface, edge: &mt::Edge| -> Result<mt::Curve> {
        let mt::Surface::BSplineSurface(s) = surface else {
            return Err(unsupported("fan spline"));
        };
        let nu = s.control_points().len();
        let nv = s.control_points().first().map_or(0, Vec::len);
        if nu == 0 || nv == 0 {
            return Err(unsupported("fan control net"));
        }
        for c in [s.row_curve(0), s.row_curve(nv - 1), s.column_curve(0), s.column_curve(nu - 1)] {
            for c in [c.clone(), c.inverse()] {
                let (a, b) = c.range_tuple();
                if from_p3(c.subs(a)).dist(vtx(edge.front())) < 1e-7 && from_p3(c.subs(b)).dist(vtx(edge.back())) < 1e-7 {
                    return Ok(mt::Curve::BSplineCurve(c));
                }
            }
        }
        Err(unsupported("fan boundary correspondence"))
    };
    for i in 1..fitted.len() {
        let edge = seams.get(i).ok_or_else(|| unsupported("fan seam"))?;
        let ca = boundary_curve(&fitted[i - 1].1, edge)?;
        let cb = boundary_curve(&fitted[i].1, edge)?;
        let (mt::Curve::BSplineCurve(a), mt::Curve::BSplineCurve(b)) = (&ca, &cb) else {
            return Err(unsupported("fan boundary spline"));
        };
        if a.knot_vec().len() != b.knot_vec().len()
            || a.control_points().len() != b.control_points().len()
            || a.knot_vec().iter().zip(b.knot_vec().iter()).any(|(a, b)| (a - b).abs() > 1e-14)
            || a.control_points().iter().zip(b.control_points()).any(|(a, b)| from_p3(*a).dist(from_p3(*b)) > 1e-8)
        {
            return Err(unsupported("fan boundary control hulls disagree"));
        }
        let seam = mt::Edge::try_new(edge.front(), edge.back(), ca).map_err(|e| KernelError::Failed(format!("fan seam: {e}")))?;
        fitted[i - 1].0[3] = seam.inverse();
        fitted[i].0[1] = seam;
    }
    crate::blend_corner::setback::partition::check(&fitted.iter().map(|(wire, _)| wire.clone()).collect::<Vec<_>>(), out, size)?;
    for (wire, surface) in fitted {
        patches.push(mt::Face::try_new(vec![wire.into()], surface).map_err(|e| KernelError::Failed(format!("transition fan patch: {e}")))?);
    }
    Ok((sections, patches))
}
