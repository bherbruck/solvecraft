//! Meshing adapter: spline bases have compact support. Skipping exactly zero
//! weights preserves the surface while avoiding work proportional to its entire
//! control net at every interior grid point.
use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, *};

#[derive(Clone)]
pub(crate) struct Surface(pub mt::Surface);

fn derivative(s: &mt::BSplineSurface<mt::Point3>, u: f64, v: f64, along_u: bool) -> mt::Vector3 {
    let (ku, kv) = s.knot_vecs();
    let (pu, pv) = s.degrees();
    let (knots, degree, t, other_knots, other_degree, other_t) = if along_u { (ku, pu, u, kv, pv, v) } else { (kv, pv, v, ku, pu, u) };
    let mut result = mt::Vector3::new(0.0, 0.0, 0.0);
    let Some(lower_degree) = degree.checked_sub(1) else { return result };
    let other: Vec<_> = other_knots.bspline_basis_functions(other_degree, other_t).into_iter().enumerate().filter(|(_, b)| *b != 0.0).collect();
    let point = |i: usize, j: usize| {
        let (i, j) = if along_u { (i, j) } else { (j, i) };
        s.control_points().get(i).and_then(|row| row.get(j)).copied().unwrap_or(mt::Point3::new(0.0, 0.0, 0.0))
    };
    for (i, b) in knots.bspline_basis_functions(lower_degree, t).into_iter().enumerate() {
        if b == 0.0 {
            continue;
        }
        let (Some(a), Some(end)) = (knots.get(i), knots.get(i + degree)) else { continue };
        let den = end - a;
        // Match truck's derivative convention at numerically collapsed spans.
        if den.so_small() {
            continue;
        }
        for (j, other) in &other {
            let previous = i.checked_sub(1).map(|i| point(i, *j)).unwrap_or(mt::Point3::new(0.0, 0.0, 0.0));
            result += (point(i, *j) - previous) * (b * other / den);
        }
    }
    result * degree as f64
}

impl ParametricSurface for Surface {
    type Point = mt::Point3;
    type Vector = mt::Vector3;
    fn subs(&self, u: f64, v: f64) -> mt::Point3 {
        let mt::Surface::BSplineSurface(s) = &self.0 else { return self.0.subs(u, v) };
        let (ku, kv) = s.knot_vecs();
        let (pu, pv) = s.degrees();
        let bu = ku.bspline_basis_functions(pu, u);
        let bv: Vec<_> = kv.bspline_basis_functions(pv, v).into_iter().enumerate().filter(|(_, b)| *b != 0.0).collect();
        let mut point = mt::Point3::new(0.0, 0.0, 0.0);
        for (row, a) in s.control_points().iter().zip(bu) {
            if a == 0.0 {
                continue;
            }
            for (j, b) in &bv {
                if let Some(p) = row.get(*j) {
                    point.x += p.x * (a * b);
                    point.y += p.y * (a * b);
                    point.z += p.z * (a * b);
                }
            }
        }
        point
    }
    fn uder(&self, u: f64, v: f64) -> mt::Vector3 {
        match &self.0 {
            mt::Surface::BSplineSurface(s) => derivative(s, u, v, true),
            _ => self.0.uder(u, v),
        }
    }
    fn vder(&self, u: f64, v: f64) -> mt::Vector3 {
        match &self.0 {
            mt::Surface::BSplineSurface(s) => derivative(s, u, v, false),
            _ => self.0.vder(u, v),
        }
    }
    fn uuder(&self, u: f64, v: f64) -> mt::Vector3 {
        self.0.uuder(u, v)
    }
    fn uvder(&self, u: f64, v: f64) -> mt::Vector3 {
        self.0.uvder(u, v)
    }
    fn vvder(&self, u: f64, v: f64) -> mt::Vector3 {
        self.0.vvder(u, v)
    }
    fn parameter_range(&self) -> (ParameterRange, ParameterRange) {
        self.0.parameter_range()
    }
    fn u_period(&self) -> Option<f64> {
        self.0.u_period()
    }
    fn v_period(&self) -> Option<f64> {
        self.0.v_period()
    }
}
impl ParametricSurface3D for Surface {
    fn normal(&self, u: f64, v: f64) -> mt::Vector3 {
        let mt::Surface::BSplineSurface(s) = &self.0 else { return self.0.normal(u, v) };
        let (du, dv) = (self.uder(u, v), self.vder(u, v));
        let normal = du.cross(dv);
        let (ur, vr) = s.range_tuple();
        let boundary = u.is_finite() && v.is_finite() && (u == ur.0 || u == ur.1 || v == vr.0 || v == vr.1);
        // Coincident endpoint controls can differ by rounding after placement.
        // Their tiny nonzero derivative is not a meaningful limiting normal.
        let roundoff = if boundary { self.subs(u, v).to_vec().magnitude().max(1.0) * f64::EPSILON * 8192.0 } else { 0.0 };
        let singular = du.magnitude2() <= roundoff * roundoff || dv.magnitude2() <= roundoff * roundoff;
        if !singular && normal.magnitude2() > 0.0 && normal.magnitude2().is_finite() {
            return normal.normalize();
        }
        // At an isolated singular corner the surface has directional limiting
        // normals. Use the interior limit for its display vertex; a surface
        // that remains degenerate still returns the original invalid normal.
        if boundary {
            let inward = |t: f64, (a, b): (f64, f64)| (t + if t < (a + b) * 0.5 { 1e-3 * (b - a) } else { -1e-3 * (b - a) }).clamp(a, b);
            let (u, v) = (inward(u, ur), inward(v, vr));
            let limit = self.uder(u, v).cross(self.vder(u, v));
            if limit.magnitude2() > 0.0 && limit.magnitude2().is_finite() {
                return limit.normalize();
            }
        }
        normal.normalize()
    }
}
impl ParameterDivision2D for Surface {
    fn parameter_division(&self, range: ((f64, f64), (f64, f64)), tol: f64) -> (Vec<f64>, Vec<f64>) {
        self.0.parameter_division(range, tol)
    }
}
impl SearchParameter<D2> for Surface {
    type Point = mt::Point3;
    fn search_parameter<H: Into<SPHint2D>>(&self, p: mt::Point3, hint: H, trials: usize) -> Option<(f64, f64)> {
        if matches!(&self.0, mt::Surface::BSplineSurface(s) if matches!(s.degrees(), (3, 3) | (5, 5))) {
            // Robust triangulation tries this exact search before the nearest
            // search. Both must use the bounded solver: an unconstrained root
            // outside a singular patch can corrupt its boundary triangulation.
            let (u, v) = self.search_nearest_parameter(p, hint, trials)?;
            return self.subs(u, v).near(&p).then_some((u, v));
        }
        if let mt::Surface::BSplineSurface(s) = &self.0 {
            let seed = sparse_search_seed(self, s.range_tuple(), p, hint.into());
            return mt::algo::surface::search_parameter3d(self, p, seed, trials);
        }
        self.0.search_parameter(p, hint, trials)
    }
}
impl SearchNearestParameter<D2> for Surface {
    type Point = mt::Point3;
    fn search_nearest_parameter<H: Into<SPHint2D>>(&self, p: mt::Point3, hint: H, trials: usize) -> Option<(f64, f64)> {
        let hint = hint.into();
        let mt::Surface::BSplineSurface(s) = &self.0 else { return self.0.search_nearest_parameter(p, hint, trials) };
        if !matches!(s.degrees(), (3, 3) | (5, 5)) {
            let seed = sparse_search_seed(self, s.range_tuple(), p, hint);
            return mt::algo::surface::search_nearest_parameter(self, p, seed, trials);
        }
        if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
            return None;
        }
        let (ur, vr) = s.range_tuple();
        let boundary_tol = (p.to_vec().magnitude().max(1.0) * f64::EPSILON * 128.0).max(1e-12);
        // At a cubic pole, indistinguishable points can have very different
        // parameters. Prefer the exact endpoint and its stable limiting normal.
        for u in [ur.0, ur.1] {
            for v in [vr.0, vr.1] {
                if self.subs(u, v).distance2(p) <= boundary_tol * boundary_tol {
                    return Some((u, v));
                }
            }
        }
        let clamp = |(u, v): (f64, f64)| (u.clamp(ur.0, ur.1), v.clamp(vr.0, vr.1));
        let improve = |(mut u, mut v): (f64, f64)| {
            for _ in 0..trials.min(100) {
                let q = self.subs(u, v) - p;
                let du = self.uder(u, v);
                let dv = self.vder(u, v);
                let a = du.magnitude2();
                let b = du.dot(dv);
                let c = dv.magnitude2();
                let det = a * c - b * b;
                if !det.is_finite() || det <= 1e-24 * a * c {
                    break;
                }
                let su = (c * du.dot(q) - b * dv.dot(q)) / det;
                let sv = (a * dv.dot(q) - b * du.dot(q)) / det;
                if !su.is_finite() || !sv.is_finite() {
                    break;
                }
                let old = q.magnitude2();
                let mut scale = 1.0;
                let mut next = (u, v);
                for _ in 0..20 {
                    let candidate = clamp((u - scale * su, v - scale * sv));
                    if self.subs(candidate.0, candidate.1).distance2(p) < old {
                        next = candidate;
                        break;
                    }
                    scale *= 0.5;
                }
                let delta = (next.0 - u).abs() + (next.1 - v).abs();
                (u, v) = next;
                if delta < 1e-13 {
                    break;
                }
            }
            (u, v)
        };
        let seed = match hint {
            SPHint2D::Parameter(u, v) if u.is_finite() && v.is_finite() => clamp((u, v)),
            _ => mt::algo::surface::presearch(self, p, (ur, vr), 12),
        };
        let first = Some(improve(seed));
        let valid = |(u, v): (f64, f64)| u.is_finite() && v.is_finite() && u >= ur.0 && u <= ur.1 && v >= vr.0 && v <= vr.1;
        let distance = |(u, v)| self.subs(u, v).distance2(p);
        let mut best = first.filter(|uv| valid(*uv));
        let mut consider = |uv| {
            if valid(uv) && distance(uv).is_finite() && best.is_none_or(|old| distance(uv) < distance(old)) {
                best = Some(uv);
            }
        };
        // A grid seed exactly on a collapsed boundary has a zero derivative,
        // so Newton cannot leave it even when the nearest point is inside.
        // Try one bounded interior seed as well; keep the better result.
        let inset = |t: f64, (a, b): (f64, f64)| t.clamp(a + (b - a) / 1024.0, b - (b - a) / 1024.0);
        let interior = (inset(seed.0, ur), inset(seed.1, vr));
        if interior != seed && first.is_some_and(|uv| distance(uv) > boundary_tol * boundary_tol) {
            consider(improve(interior));
        }
        if !matches!(hint, SPHint2D::None) {
            consider(improve(mt::algo::surface::presearch(self, p, (ur, vr), 12)));
        }
        // Prefer an incident boundary over a numerically equivalent interior
        // root. Otherwise roundoff can turn a straight parameter boundary into
        // a thin zigzag, producing overlapping sliver triangles along a seam.
        let mut boundary_best = None;
        let mut boundary = |uv| {
            consider(uv);
            if valid(uv) && distance(uv) <= boundary_tol * boundary_tol && boundary_best.is_none_or(|old| distance(uv) < distance(old)) {
                boundary_best = Some(uv);
            }
        };
        // A singular boundary can trap Newton's iteration. Search its four
        // regular one-dimensional boundary curves as independent candidates.
        if s.is_clamped() {
            let rows = s.control_points().len();
            let columns = s.control_points().first().map_or(0, Vec::len);
            if let (Some(last_u), Some(last_v)) = (rows.checked_sub(1), columns.checked_sub(1)) {
                for (curve, along_u, fixed) in [
                    (s.row_curve(0), true, vr.0),
                    (s.row_curve(last_v), true, vr.1),
                    (s.column_curve(0), false, ur.0),
                    (s.column_curve(last_u), false, ur.1),
                ] {
                    let (a, b) = curve.range_tuple();
                    let uv = |t| if along_u { (t, fixed) } else { (fixed, t) };
                    boundary(uv(a));
                    boundary(uv(b));
                    if let Some(mut t) = curve.search_nearest_parameter(p, None, trials) {
                        // Polish the one-dimensional boundary solve: the generic
                        // Newton stopping threshold is too coarse at a flat pole.
                        for _ in 0..16 {
                            let d = curve.der(t);
                            let norm = d.magnitude2();
                            if !norm.is_finite() || norm <= 0.0 {
                                break;
                            }
                            let next = (t - (curve.subs(t) - p).dot(d) / norm).clamp(a, b);
                            if !next.is_finite() {
                                break;
                            }
                            let delta = (next - t).abs();
                            t = next;
                            if delta <= 1e-14 * (b - a) {
                                break;
                            }
                        }
                        boundary(uv(t));
                    }
                }
            }
        }
        boundary_best.or(best).map(|(u, v)| {
            // Nearly identical trim points can invert just inside an edge.
            // Snap only a parameter already within 1e-8 of the domain boundary,
            // and only when its world-space movement is roundoff-sized. This
            // keeps duplicate seam samples from forming crossing sliver loops.
            let snap = |t: f64, (a, b): (f64, f64)| {
                if (t - a).abs() <= (b - a) * 1e-8 {
                    a
                } else if (t - b).abs() <= (b - a) * 1e-8 {
                    b
                } else {
                    t
                }
            };
            let snapped = (snap(u, ur), snap(v, vr));
            let movement = self.subs(snapped.0, snapped.1).distance2(self.subs(u, v));
            if movement <= (boundary_tol * 64.0).powi(2) { snapped } else { (u, v) }
        })
    }
}

// Keep truck's search and 50-division seed grid, but evaluate that grid through
// the compact-support adapter. Delegating to the raw spline here made every
// normal sample on a cubic transition scan its entire control net thousands
// of times, including intermediate bodies used to resolve later edge names.
fn sparse_search_seed(surface: &Surface, range: ((f64, f64), (f64, f64)), p: mt::Point3, hint: SPHint2D) -> (f64, f64) {
    match hint {
        SPHint2D::Parameter(u, v) => (u, v),
        SPHint2D::Range(u, v) => mt::algo::surface::presearch(surface, p, (u, v), 50),
        SPHint2D::None => mt::algo::surface::presearch(surface, p, range, 50),
    }
}

/// Invert the surface in a fixed display projection. Refining in UV space
/// after projected triangulation can reverse a triangle on a distorted chart.
pub(crate) fn projected_parameter(surface: &Surface, point: mt::Point3, direction: Vec3, seed: (f64, f64)) -> Option<(f64, f64)> {
    if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() || !seed.0.is_finite() || !seed.1.is_finite() {
        return None;
    }
    let direction = direction.normalized()?;
    let x = direction.any_perp();
    let y = direction.cross(x);
    let x = mt::Vector3::new(x.x, x.y, x.z);
    let y = mt::Vector3::new(y.x, y.y, y.z);
    let (ur, vr) = surface.try_range_tuple();
    let (ur, vr) = (ur?, vr?);
    if ![ur.0, ur.1, vr.0, vr.1].iter().all(|x| x.is_finite()) || ur.0 >= ur.1 || vr.0 >= vr.1 {
        return None;
    }
    let mut uv = (seed.0.clamp(ur.0, ur.1), seed.1.clamp(vr.0, vr.1));
    let residual = |(u, v)| {
        let d = surface.subs(u, v) - point;
        d.dot(x).powi(2) + d.dot(y).powi(2)
    };
    for _ in 0..24 {
        let d = surface.subs(uv.0, uv.1) - point;
        if residual(uv) < 1e-20 {
            return Some(uv);
        }
        let (du, dv) = (surface.uder(uv.0, uv.1), surface.vder(uv.0, uv.1));
        let (a, b, c, e) = (du.dot(x), dv.dot(x), du.dot(y), dv.dot(y));
        let det = a * e - b * c;
        if !det.is_finite() || det.abs() <= 1e-14 * a.hypot(c) * b.hypot(e) {
            return None;
        }
        let (su, sv) = ((d.dot(x) * e - d.dot(y) * b) / det, (d.dot(y) * a - d.dot(x) * c) / det);
        if !su.is_finite() || !sv.is_finite() {
            return None;
        }
        let old = residual(uv);
        let mut scale = 1.0;
        let mut next = None;
        for _ in 0..16 {
            let candidate = ((uv.0 - scale * su).clamp(ur.0, ur.1), (uv.1 - scale * sv).clamp(vr.0, vr.1));
            if residual(candidate) < old {
                next = Some(candidate);
                break;
            }
            scale *= 0.5;
        }
        uv = next?;
    }
    (residual(uv) < 1e-20).then_some(uv)
}

pub(crate) fn display_projection(surface: &mt::BSplineSurface<mt::Point3>, mesh_normals: &[mt::Vector3]) -> Option<Vec3> {
    if !matches!(surface.degrees(), (3, 3) | (5, 5)) || surface.control_points().len() > 64 || surface.control_points().iter().any(|r| r.len() > 64) {
        return None;
    }
    let origin = *surface.control_points().first()?.first()?;
    let local = mt::BSplineSurface::try_new(
        (surface.uknot_vec().clone(), surface.vknot_vec().clone()),
        surface
            .control_points()
            .iter()
            .map(|row| row.iter().map(|p| mt::Point3::new(p.x - origin.x, p.y - origin.y, p.z - origin.z)).collect())
            .collect(),
    )
    .ok()?;
    let (ur, vr) = local.range_tuple();
    if ur != (0.0, 1.0) || vr != (0.0, 1.0) {
        return None;
    }
    let evaluated = crate::body::evaluation::Surface(mt::Surface::BSplineSurface(local));
    let mut normals = Vec::new();
    for i in 0..=16 {
        for j in 0..=16 {
            if (i == 0 || i == 16) && (j == 0 || j == 16) {
                continue;
            }
            let n = evaluated.normal(i as f64 / 16.0, j as f64 / 16.0);
            if let Some(n) = Vec3::new(n.x, n.y, n.z).normalized() {
                normals.push(n);
            }
        }
    }
    normals.extend(mesh_normals.iter().filter_map(|n| Vec3::new(n.x, n.y, n.z).normalized()));
    let mut direction = normals.iter().copied().fold(Vec3::ZERO, |a, b| a + b).normalized()?;
    // This only chooses how to connect existing display vertices; construction
    // still requires its independent Jacobian and containment certificates.
    for _ in 0..1024 {
        let worst = normals.iter().min_by(|a, b| a.dot(direction).total_cmp(&b.dot(direction)))?;
        let dot = worst.dot(direction);
        if dot >= 0.01 {
            break;
        }
        direction = (direction + *worst * (0.011 - dot)).normalized()?;
    }
    normals.iter().all(|n| n.dot(direction) >= 0.01).then_some(direction)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projected_display_mesh_removes_crossing_diagonals_without_moving_vertices() {
        use truck_meshalgo::prelude::{Faces, PolygonMesh, StandardAttributes, retriangulate_projected};
        let positions = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.8, 0.7), (0.2, 0.3)]
            .into_iter()
            .map(|(x, y)| mt::Point3::new(x, y, 0.0))
            .collect::<Vec<_>>();
        // Two interior vertices exchange sides in projection. The original
        // parameter diagonals overlap, although the surface itself is flat.
        let triangles = [[0, 1, 5], [0, 5, 4], [0, 4, 3], [1, 2, 5], [2, 4, 5], [2, 3, 4]];
        let mesh = PolygonMesh::new(
            StandardAttributes { positions, normals: vec![mt::Vector3::unit_z(); 6], uv_coords: vec![mt::Vector2::new(0.0, 0.0); 6] },
            Faces::from_tri_and_quad_faces(triangles.into_iter().map(|t| t.map(|i| [i, i, i].into())).collect(), Vec::new()),
        );
        let fixed = retriangulate_projected(&mesh, mt::Vector3::unit_z()).unwrap();
        assert_eq!(fixed.positions(), mesh.positions());
        assert_eq!(fixed.normals(), mesh.normals());
        let mut area = 0.0;
        for triangle in fixed.faces().triangle_iter() {
            let [a, b, c] = triangle.map(|v| fixed.positions()[v.pos]);
            let signed = (b - a).cross(c - a).z * 0.5;
            assert!(signed > 0.0);
            area += signed;
        }
        assert!((area - 1.0).abs() < 1e-12);
        assert!(retriangulate_projected(&mesh, -mt::Vector3::unit_z()).is_none());
        assert!(retriangulate_projected(&mesh, mt::Vector3::zero()).is_none());
    }

    #[test]
    fn bounded_projection_recovers_from_a_singular_or_outside_hint() {
        // x=u³, y=v³: the origin is stationary in both parameters.
        let cubic = [0.0, 0.0, 0.0, 0.1, 0.4, 1.0];
        let ctrl = cubic.iter().map(|x| cubic.iter().map(|y| mt::Point3::new(*x, *y, 0.0)).collect()).collect();
        let knots = mt::KnotVec::bezier_knot(5);
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), ctrl)));
        for p in [mt::Point3::new(0.0, 0.5, 0.0), mt::Point3::new(0.125, 0.343, 0.0), mt::Point3::new(0.0, 0.0, 0.0)] {
            for hint in [(0.0, 0.0), (9.0, -9.0)] {
                let (u, v) = surface.search_nearest_parameter(p, hint, 100).unwrap();
                assert!((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v));
                assert!(surface.subs(u, v).distance(p) < 1e-10);
                let (u, v) = surface.search_parameter(p, hint, 100).unwrap();
                assert!((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v));
                assert!(surface.subs(u, v).distance(p) < 1e-10);
            }
        }
        assert!(surface.search_parameter(mt::Point3::new(-0.125, 0.5, 0.0), None, 100).is_none());
        assert_eq!(surface.search_nearest_parameter(mt::Point3::new(1e-14, 1e-14, 0.0), (0.001, 0.001), 100), Some((0.0, 0.0)));
        assert!(surface.search_nearest_parameter(mt::Point3::new(f64::NAN, 0.0, 0.0), None, 100).is_none());
    }

    #[test]
    fn projected_refinement_uses_physical_not_parameter_midpoints() {
        let cubic = [0.0, 0.0, 0.0, 0.1, 0.4, 1.0];
        let controls = cubic.iter().map(|x| cubic.iter().map(|y| mt::Point3::new(*x, *y, 0.0)).collect()).collect();
        let knots = mt::KnotVec::bezier_knot(5);
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), controls)));
        let p = mt::Point3::new(0.5, 0.5, 0.2);
        let (u, v) = projected_parameter(&surface, p, Vec3::Z, (0.5, 0.5)).unwrap();
        assert!((u - 0.5_f64.cbrt()).abs() < 1e-9 && (v - u).abs() < 1e-9);
        assert!(projected_parameter(&surface, p, Vec3::ZERO, (0.5, 0.5)).is_none());
        assert!(projected_parameter(&surface, p, Vec3::Z, (f64::NAN, 0.5)).is_none());
        assert!(projected_parameter(&surface, mt::Point3::new(2.0, 0.5, 0.0), Vec3::Z, (0.5, 0.5)).is_none());
    }

    #[test]
    fn cubic_and_quintic_search_resolve_narrow_boundary_strips() {
        for degree in [3_usize, 5] {
            let power3 = |i: usize| if i < 3 { 0.0 } else { (i * (i - 1) * (i - 2)) as f64 / (degree * (degree - 1) * (degree - 2)) as f64 };
            let knots = mt::KnotVec::bezier_knot(degree);
            let controls = (0..=degree)
                .map(|i| {
                    (0..=degree)
                        .map(|j| {
                            let z = 0.01 * (i * i.saturating_sub(1)) as f64 / (degree * (degree - 1)) as f64 * j as f64 / degree as f64;
                            mt::Point3::new(700.0 + 10.0 * power3(i), -150.0 + 10.0 * power3(j), 730.0 + z)
                        })
                        .collect()
                })
                .collect();
            let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), controls)));
            for (u, v) in [(0.85, 0.004), (0.02, 0.7), (0.7, 0.8)] {
                let point = surface.subs(u, v);
                let (a, b) = surface.search_nearest_parameter(point, None, 100).unwrap();
                assert!(
                    surface.subs(a, b).distance(point) < 1e-9,
                    "degree {degree}: ({u},{v}) resolved as ({a},{b}), distance {}",
                    surface.subs(a, b).distance(point)
                );
                assert!(
                    surface.normal(a, b).dot(surface.normal(u, v)) > 0.01_f64.to_radians().cos(),
                    "degree {degree}: ({u},{v}) resolved as ({a},{b})"
                );
            }
        }
    }

    #[test]
    fn roundoff_near_a_trim_boundary_does_not_create_a_sliver() {
        let knots = mt::KnotVec::bezier_knot(5);
        for origin in [mt::Point3::new(0.0, 0.0, 0.0), mt::Point3::new(100.0, 200.0, 700.0)] {
            let controls = (0..6).map(|i| (0..6).map(|j| origin + mt::Vector3::new(i as f64 / 5.0, j as f64 / 5.0, 0.0)).collect()).collect();
            let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots.clone()), controls)));
            for (u, v) in [(2e-11, 0.4), (1.0 - 2e-11, 0.4), (0.4, 2e-11), (0.4, 1.0 - 2e-11)] {
                let p = surface.subs(u, v);
                let result = surface.search_nearest_parameter(p, (u, v), 100).unwrap();
                assert!(result.0 == 0.0 || result.0 == 1.0 || result.1 == 0.0 || result.1 == 1.0);
                assert!(surface.subs(result.0, result.1).distance(p) < 1e-9);
            }
            let p = surface.subs(1e-5, 0.4);
            let result = surface.search_nearest_parameter(p, None, 100).unwrap();
            assert!((result.0 - 1e-5).abs() < 1e-10, "a distinct interior point must stay inside");
        }
    }

    #[test]
    fn meshing_checks_normal_error_on_a_narrow_curved_strip() {
        use truck_meshalgo::prelude::*;
        let knots = mt::KnotVec::bezier_knot(5);
        // A narrow bump, z=3u²(1-u)³v²(1-v)², lies below the chord
        // tolerance but turns the normal substantially. All four boundary
        // curves and their normals remain flat, isolating interior shading.
        let controls = (0..6)
            .map(|i| {
                (0..6).map(|j| mt::Point3::new(i as f64 / 50.0, j as f64 / 5.0, if i == 2 && (j == 2 || j == 3) { 0.03 } else { 0.0 })).collect()
            })
            .collect();
        let patch = mt::BSplineSurface::new((knots.clone(), knots), controls);
        let surface = Surface(mt::Surface::BSplineSurface(patch.clone()));
        let vertices = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(u, v)| mt::Vertex::new(surface.subs(u, v)));
        let curves = [patch.row_curve(0), patch.column_curve(5), patch.row_curve(5).inverse(), patch.column_curve(0).inverse()];
        let wire =
            mt::Wire::from_iter((0..4).map(|i| mt::Edge::new(&vertices[i], &vertices[(i + 1) % 4], mt::Curve::BSplineCurve(curves[i].clone()))));
        let face = truck_topology::Face::new(vec![wire], surface.clone());
        let mesh = truck_topology::Shell::from(vec![face]).robust_triangulation(0.02);
        let polygon = mesh.face_iter().next().unwrap().surface().unwrap();
        let triangles = crate::body::refined_quality(&polygon, &surface.0, 0.02, true, true).unwrap();
        assert_eq!(
            triangles,
            crate::body::refined_quality(&polygon, &surface.0, 0.02, false, true).unwrap(),
            "caching must preserve every refinement decision"
        );
        let mut worst = 0.0_f64;
        for t in &triangles {
            let p = t.iter().map(|(p, _)| *p).fold(Vec3::ZERO, |a, b| a + b) / 3.0;
            let n = t.iter().map(|(_, n)| *n).fold(Vec3::ZERO, |a, b| a + b).normalized().unwrap();
            let exact = surface.normal(p.x / 0.1, p.y);
            worst = worst.max(n.dot(Vec3::new(exact.x, exact.y, exact.z)).clamp(-1.0, 1.0).acos().to_degrees());
        }
        assert!(worst < 2.0, "interpolated normal differs by {worst} degrees");
        assert!(triangles.len() < 1000, "normal refinement must stay bounded");
    }

    #[test]
    fn interior_fold_does_not_acquire_a_fallback_normal() {
        let knots = mt::KnotVec::bezier_knot(2);
        let ctrl = [0.25, -0.25, 0.25].into_iter().map(|x| (0..3).map(|j| mt::Point3::new(x, j as f64 / 2.0, 0.0)).collect()).collect();
        let s = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), ctrl)));
        assert!(!s.normal(0.5, 0.5).magnitude2().is_finite());
    }

    #[test]
    fn singular_corner_uses_a_finite_interior_normal() {
        let knots = mt::KnotVec::bezier_knot(2);
        let ctrl =
            (0..3).map(|i| (0..3).map(|j| mt::Point3::new(if i == 2 { 1.0 } else { 0.0 }, if j == 2 { 1.0 } else { 0.0 }, 0.0)).collect()).collect();
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), ctrl)));
        assert_eq!(surface.normal(0.0, 0.0), mt::Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(surface.normal(0.0, 0.5), mt::Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn placed_singular_corner_ignores_control_roundoff() {
        let cubic = [0.0, 0.0, 0.0, 0.1, 0.4, 1.0];
        let mut ctrl: Vec<Vec<_>> = cubic.iter().map(|x| cubic.iter().map(|y| mt::Point3::new(100.0 + x, 200.0 + y, 700.0)).collect()).collect();
        ctrl[1][0].x += 1e-12;
        ctrl[0][1].z += 1e-12;
        let knots = mt::KnotVec::bezier_knot(5);
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), ctrl)));
        assert!(surface.normal(0.0, 0.0).dot(mt::Vector3::unit_z()) > 0.99999);
    }
    #[test]
    fn sparse_evaluation_preserves_knots_and_domain_endpoints() {
        let knots = mt::KnotVec::from(vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0]);
        let control = (0..8).map(|i| (0..8).map(|j| mt::Point3::new(i as f64, j as f64, ((i * 7 + j) as f64).sin())).collect()).collect();
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), control)));
        for u in [0.0, 0.13, 0.2, 0.4, 0.57, 0.7, 1.0] {
            for v in [0.0, 0.13, 0.2, 0.4, 0.57, 0.7, 1.0] {
                assert_eq!(surface.subs(u, v), surface.0.subs(u, v));
                assert!((surface.uder(u, v) - surface.0.uder(u, v)).magnitude() < 1e-12);
                assert!((surface.vder(u, v) - surface.0.vder(u, v)).magnitude() < 1e-12);
                assert!((surface.normal(u, v) - surface.0.normal(u, v)).magnitude() < 1e-12);
            }
        }
    }

    #[test]
    fn collapsed_knot_spans_keep_the_original_derivative_convention() {
        let knots = mt::KnotVec::from(vec![0.0, 0.0, 0.0, 0.0, 1e-8, 0.4, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0]);
        let control = (0..8).map(|i| (0..8).map(|j| mt::Point3::new(i as f64, j as f64, ((i * 7 + j) as f64).sin())).collect()).collect();
        let surface = Surface(mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), control)));
        for u in [0.0, 0.5e-8, 1e-8, 0.2, 1.0] {
            for v in [0.0, 0.5e-8, 1e-8, 0.2, 1.0] {
                assert_eq!(surface.subs(u, v), surface.0.subs(u, v));
                assert!((surface.uder(u, v) - surface.0.uder(u, v)).magnitude() < 1e-12);
                assert!((surface.vder(u, v) - surface.0.vder(u, v)).magnitude() < 1e-12);
            }
        }
    }
}
