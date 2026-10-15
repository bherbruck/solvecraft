//! Local end patches for a cylindrical fillet whose section misses a fan seam.
//! The selected edge retains its constant-radius cylinder; only its end is capped.

use super::*;
use crate::blend_corner::setback::{Boundary, Normal};
use solidcraft_geom::Aabb3;
mod fan;

pub(super) struct End {
    pub a: mt::Vertex,
    pub b: mt::Vertex,
    pub sections: Vec<mt::Edge>,
    pub removed: Vec<mt::EdgeID>,
    pub replacements: HashMap<mt::EdgeID, mt::Edge>,
    pub connections: Vec<(usize, mt::Edge)>,
    pub patches: Vec<mt::Face>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    faces: &[&mt::Face],
    fan: &[(mt::Edge, usize)],
    vertex: &mt::Vertex,
    p: Vec3,
    inward: Vec3,
    t1: Vec3,
    t2: Vec3,
    n1: Vec3,
    n2: Vec3,
    sign: f64,
    s: f64,
    r: f64,
    w: f64,
    size: f64,
    tol: f64,
    bounds: Aabb3,
    fa: usize,
    fb: usize,
) -> Result<End> {
    if sign <= 0.0 || fan.len() < 2 || fan.len() > 16 {
        return Err(unsupported("this transition needs a concave endpoint fan"));
    }
    let (fan, mut removed) = expand_fan(faces, fan, vertex, p, t1, t2, s, tol, fa, fb)?;
    let curved = fan.iter().take(fan.len() - 1).any(|(_, fi)| faces.get(*fi).is_none_or(|f| plane_normal(f).is_none()));
    let center = p + t1 * s + n1 * (sign * r);
    let mut points = Vec::new();
    let mut crease = false;
    let mut replacements = HashMap::new();
    for (i, (seam, _)) in fan.iter().enumerate() {
        let oriented = seam.clone();
        let curve = oriented.oriented_curve();
        let (_, last) = curve.range_tuple();
        if curved && i > 0 && i + 1 < fan.len() {
            let point = oriented.back();
            let previous = fan.get(i - 1).and_then(|(_, fi)| faces.get(*fi)).ok_or_else(|| unsupported("transition fan face"))?;
            let next = fan.get(i).and_then(|(_, fi)| faces.get(*fi)).ok_or_else(|| unsupported("transition fan face"))?;
            let normal = |face: &mt::Face| -> Result<Vec3> {
                let surface = face.oriented_surface();
                let (u, v) = surface.search_nearest_parameter(point.point(), None, 100).ok_or_else(|| unsupported("transition seam normal"))?;
                let n = surface.normal(u, v);
                Ok(Vec3::new(n.x, n.y, n.z))
            };
            if normal(previous)?.dot(normal(next)?) < 1.0 - 1e-6 {
                crease = true;
            }
            removed.push(seam.id());
            points.push(point.clone());
            continue;
        }
        let (t, q) = if i == 0 || i + 1 == fan.len() {
            let side = if i == 0 { t1 } else { t2 };
            crossing(&curve, |q| (q - p).dot(side) - s, tol)?
        } else {
            let mt::Curve::Line(_) = curve else {
                return Err(unsupported("a curved transition seam"));
            };
            // This is a forward, bounded move on the original seam, never the
            // unchecked backward extension that can spike out of a convex joint.
            let delta = vtx(oriented.back()) - p;
            let radial = delta - inward * delta.dot(inward);
            let offset = p - center;
            let offset = offset - inward * offset.dot(inward);
            let u = -offset.dot(radial) / radial.dot(radial);
            if !(u.is_finite() && u > 1e-6 && u < 1.0 - 1e-6) {
                return Err(unsupported("transition would consume the next vertex"));
            }
            let (first, last) = curve.range_tuple();
            let t = first + u * (last - first);
            (t, from_p3(curve.subs(t)))
        };
        let nv = builder::vertex(p3(q));
        let ne = mt::Edge::try_new(&nv, oriented.back(), trim(curve, t, last)).map_err(|e| KernelError::Failed(format!("transition seam: {e}")))?;
        replacements.insert(seam.id(), if oriented.front() == seam.absolute_front() { ne } else { ne.inverse() });
        points.push(nv);
    }
    // Keep curved endpoint fills local to the neighboring rounds. A long
    // unconstrained collar can overshoot the barrel and reverse its normals.
    // Sharp planar fans still need the larger setback to clear their seams.
    let collar = if curved { 0.25 * r } else { 2.0 * r };
    let h = points.iter().map(|v| (vtx(v) - p).dot(inward)).fold(0.0_f64, f64::max) + collar;
    if !h.is_finite() || h > size {
        return Err(unsupported("oversized transition setback"));
    }
    let q = p + inward * h;
    let a = builder::vertex(p3(q + t1 * s));
    let b = builder::vertex(p3(q + t2 * s));
    let section = mt::Edge::try_new(&a, &b, conic(vtx(&a), q, vtx(&b), w)).map_err(|e| KernelError::Failed(format!("transition section: {e}")))?;
    let mut wire = vec![section.inverse()];
    let mut boundaries = vec![Boundary {
        curve: section.oriented_curve().inverse(),
        normal: if sign > 0.0 { Normal::InwardCylinder { origin: center, axis: inward } } else { Normal::Cylinder { origin: center, axis: inward } },
    }];
    let first = points.first().ok_or_else(|| unsupported("empty transition fan"))?;
    let last = points.last().ok_or_else(|| unsupported("empty transition fan"))?;
    let rail_a = builder::line(&a, first);
    boundaries.push(Boundary { curve: rail_a.oriented_curve(), normal: Normal::Plane(n1) });
    wire.push(rail_a.clone());
    let mut connections = vec![(fa, rail_a)];
    for (i, pair) in points.windows(2).enumerate() {
        let [a, b] = pair else {
            return Err(unsupported("transition endpoints"));
        };
        let (_, fi) = fan.get(i).ok_or_else(|| unsupported("transition end face"))?;
        let face = faces.get(*fi).ok_or_else(|| unsupported("transition end face"))?;
        let (curve, normal) = if plane_normal(face).is_some() {
            let delta = vtx(b) - vtx(a);
            let unit = delta.normalized().ok_or_else(|| unsupported("collapsed transition end edge"))?;
            if !crate::blend::extension_fits(face, vtx(a) + unit * tol * 20.0, vtx(b) - unit * tol * 20.0, tol) {
                return Err(unsupported("transition leaves its end face"));
            }
            // Tangency to the seams makes this curve compatible with the two
            // side-wall tangent planes, while keeping the sharp floor meeting.
            (conic(vtx(a), p, vtx(b), 1.0), Normal::Free)
        } else {
            cylinder_boundary(face, vtx(a), vtx(b), tol, i == 0, i + 2 == points.len())?
        };
        let region = crate::blend_trim::FaceRegion::new(face, tol)?;
        let (lo, hi) = curve.range_tuple();
        for j in 1..256 {
            if !region.contains(curve.subs(lo + (hi - lo) * j as f64 / 256.0), tol) {
                return Err(unsupported("transition boundary leaves its trimmed face"));
            }
        }
        let edge = mt::Edge::try_new(a, b, curve).map_err(|e| KernelError::Failed(format!("transition boundary: {e}")))?;
        connections.push((*fi, edge.clone()));
        boundaries.push(Boundary { curve: edge.oriented_curve(), normal });
        wire.push(edge);
    }
    let rail_b = builder::line(last, &b);
    boundaries.push(Boundary { curve: rail_b.oriented_curve(), normal: Normal::Plane(n2) });
    wire.push(rail_b.clone());
    connections.push((fb, rail_b));
    let mut planes = vec![
        (Vec3::X, bounds.max),
        (-Vec3::X, bounds.min),
        (Vec3::Y, bounds.max),
        (-Vec3::Y, bounds.min),
        (Vec3::Z, bounds.max),
        (-Vec3::Z, bounds.min),
    ];
    // When the whole end boundary lies on the cavity side, keep the
    // complete patch there too, including points between boundary samples.
    // A convex rim can cross these planes, so its outer envelope remains the
    // applicable bound instead.
    for n in [n1, n2] {
        let inside = |q: Vec3| q.is_finite() && (q - p).dot(n) >= -tol;
        if boundaries.iter().all(|boundary| match &boundary.curve {
            mt::Curve::Line(line) => inside(from_p3(line.0)) && inside(from_p3(line.1)),
            mt::Curve::BSplineCurve(curve) => curve.control_points().iter().all(|q| inside(from_p3(*q))),
            mt::Curve::NurbsCurve(curve) => curve.control_points().iter().all(|q| q.w > 0.0 && inside(Vec3::new(q.x / q.w, q.y / q.w, q.z / q.w))),
            _ => false,
        }) {
            planes.push((-n, p));
        }
    }
    for (fi, start, end) in [(fa, vtx(&a), vtx(first)), (fb, vtx(&b), vtx(last))] {
        let face = faces.get(fi).ok_or_else(|| unsupported("transition side face"))?;
        let d = (end - start).normalized().ok_or_else(|| unsupported("collapsed transition rail"))?;
        if !crate::blend::extension_fits(face, start + d * tol * 20.0, end - d * tol * 20.0, tol) {
            return Err(unsupported("transition contact rail leaves its side face"));
        }
    }
    let mut out = n1 + n2;
    // Include the endpoint normals even for planar floors. The side-wall
    // bisector alone can project the floor fan back across the cylinder section.
    for (_, fi) in fan.iter().take(fan.len() - 1) {
        let f = faces.get(*fi).ok_or_else(|| unsupported("transition fan face"))?;
        let surface = f.oriented_surface();
        let point = points.get(points.len() / 2).ok_or_else(|| unsupported("transition midpoint"))?;
        let (u, v) = surface.search_nearest_parameter(point.point(), None, 100).ok_or_else(|| unsupported("transition direction"))?;
        let n = surface.normal(u, v);
        out += Vec3::new(n.x, n.y, n.z);
    }
    if crease || (curved && fan.len() > 2) {
        match fan::build(faces, &fan, &points, &wire, &boundaries, &section, center, inward, sign, size, &planes, out, crease) {
            Ok((sections, patches)) => return Ok(End { a, b, sections, removed, replacements, connections, patches }),
            Err(error) if crease => return Err(error),
            // A shallow rim can fold the fixed rectangular chart. A smooth
            // terminal fan also admits a single trimmed graph; that fit still
            // must pass the same position, tangency and containment checks.
            Err(_) => {}
        }
    }
    crate::blend_corner::setback::partition::check(std::slice::from_ref(&wire), out, size)?;
    let surface = crate::blend_corner::setback::transition(&boundaries, out, size, &planes)?;
    let patch = mt::Face::try_new(vec![wire.into()], surface).map_err(|e| KernelError::Failed(format!("transition patch: {e}")))?;
    Ok(End { a, b, sections: vec![section], removed, replacements, connections, patches: vec![patch] })
}

/// Exact cylindrical boundary with zero angular velocity at its endpoints.
/// It therefore joins a contact rail without imposing incompatible mixed
/// curvature on the new patch's side-wall boundary.
fn cylinder_boundary(face: &mt::Face, start: Vec3, end: Vec3, tol: f64, start_rail: bool, end_rail: bool) -> Result<(mt::Curve, Normal)> {
    use crate::offset::{Surf, surf_of};
    let Some(Surf::Cylinder { o, a, r, convex }) = surf_of(face, tol) else {
        return Err(unsupported("non-cylindrical transition end"));
    };
    let hs = (start - o).dot(a);
    let he = (end - o).dot(a);
    let u = (start - o - a * hs) / r;
    let v = (end - o - a * he) / r;
    let cosine = u.dot(v).clamp(-1.0, 1.0);
    let w = ((1.0 + cosine) * 0.5).sqrt();
    if w < 1e-3 || (he - hs).abs() < tol {
        return Err(unsupported("degenerate cylindrical transition boundary"));
    }
    let circle = [h(o + u * r, 1.0), h(o + (u + v) * (r / (1.0 + cosine)), w), h(o + v * r, 1.0)];
    // Compose a quadratic homogeneous circle with t = 3s² - 2s³.
    let blend = [0.0, 0.0, 1.0, 1.0];
    let complement = [1.0, 1.0, 0.0, 0.0];
    let choose = |n: usize, k: usize| (0..k.min(n - k)).fold(1.0, |x, j| x * (n - j) as f64 / (j + 1) as f64);
    let product = |x: &[f64], y: &[f64]| {
        let (n, m) = (x.len() - 1, y.len() - 1);
        (0..=n + m)
            .map(|k| {
                x.iter()
                    .enumerate()
                    .filter_map(|(i, x)| {
                        let j = k.checked_sub(i)?;
                        y.get(j).map(|y| x * y * choose(n, i) * choose(m, j) / choose(n + m, k))
                    })
                    .sum::<f64>()
            })
            .collect::<Vec<_>>()
    };
    let coefficients = [product(&complement, &complement), product(&blend, &complement), product(&blend, &blend)];
    let mut angular = Vec::new();
    for i in 0..=6 {
        let mut q = mt::Vector4::new(0.0, 0.0, 0.0, 0.0);
        for (j, circle) in circle.iter().enumerate() {
            let value = coefficients.get(j).and_then(|c| c.get(i)).copied().ok_or_else(|| unsupported("transition curve coefficient"))?;
            q += *circle * value * if j == 1 { 2.0 } else { 1.0 };
        }
        angular.push(q);
    }
    // On a concave floor round, turn away from the wall before advancing
    // quickly toward the fan tip. Linear axial progress can force the fill to
    // bend back toward its barrel. Mirrored Bernstein controls give the same
    // physical boundary when traversed in reverse. All heights remain between
    // the endpoints and the curve stays exactly on the original cylinder.
    let midpoint = if start_rail && !end_rail {
        0.2
    } else if end_rail && !start_rail {
        0.8
    } else {
        0.5
    };
    let heights = if convex || midpoint == 0.5 { vec![hs, he] } else { vec![hs, hs + (he - hs) * midpoint, he] };
    let height_degree = heights.len() - 1;
    let degree = 6 + height_degree;
    let mut controls = Vec::new();
    for k in 0_usize..=degree {
        let mut q = mt::Vector4::new(0.0, 0.0, 0.0, 0.0);
        for (i, point) in angular.iter().enumerate() {
            let Some(j) = k.checked_sub(i) else { continue };
            let Some(height) = heights.get(j).copied() else { continue };
            q += (*point + mt::Vector4::new(a.x * height * point.w, a.y * height * point.w, a.z * height * point.w, 0.0))
                * (choose(6, i) * choose(height_degree, j) / choose(degree, k));
        }
        controls.push(q);
    }
    let curve = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(degree), controls)));
    Ok((curve, if convex { Normal::Cylinder { origin: o, axis: a } } else { Normal::InwardCylinder { origin: o, axis: a } }))
}

/// Follow a side face past short existing blend edges before placing the new
/// contact point. Each consumed edge expands the endpoint fan through the
/// intervening faces; all those faces retain a boundary on the transition.
#[allow(clippy::too_many_arguments)]
fn expand_fan(
    faces: &[&mt::Face],
    initial: &[(mt::Edge, usize)],
    vertex: &mt::Vertex,
    p: Vec3,
    t1: Vec3,
    t2: Vec3,
    setback: f64,
    tol: f64,
    fa: usize,
    fb: usize,
) -> Result<(Vec<(mt::Edge, usize)>, Vec<mt::EdgeID>)> {
    let other = |edge: &mt::Edge, current: usize| {
        faces
            .iter()
            .enumerate()
            .find(|(i, f)| *i != current && f.edge_iter().any(|e| e.id() == edge.id()))
            .map(|(i, _)| i)
            .ok_or_else(|| unsupported("open transition fan"))
    };
    let mut edges: Vec<_> = initial.iter().map(|(e, _)| if e.front() == vertex { e.clone() } else { e.inverse() }).collect();
    let mut removed = Vec::new();
    for _ in 0..32 {
        let mut changed = false;
        for (at_start, side, direction) in [(true, fa, t1), (false, fb, t2)] {
            let index = if at_start { 0 } else { edges.len().saturating_sub(1) };
            let edge = edges.get(index).cloned().ok_or_else(|| unsupported("empty transition fan"))?;
            let curve = edge.oriented_curve();
            let residual = |q: Vec3| (q - p).dot(direction) - setback;
            if crossing(&curve, residual, tol).is_ok() {
                continue;
            }
            let (lo, hi) = curve.range_tuple();
            if (0..=128).any(|i| {
                let value = residual(from_p3(curve.subs(lo + (hi - lo) * i as f64 / 128.0)));
                !value.is_finite() || value >= -tol
            }) {
                return Err(unsupported("transition contact cannot advance past this edge"));
            }
            if removed.contains(&edge.id()) {
                return Err(unsupported("cyclic transition expansion"));
            }
            let stop = other(&edge, side)?;
            let root = edge.back();
            let mut current = side;
            let mut incoming = edge.id();
            let mut chain = Vec::new();
            for _ in 0..16 {
                let face = faces.get(current).ok_or_else(|| unsupported("transition side face"))?;
                let next = face
                    .edge_iter()
                    .find(|e| e.id() != incoming && (e.front() == root || e.back() == root))
                    .ok_or_else(|| unsupported("open expanded transition fan"))?;
                let next = if next.front() == root { next } else { next.inverse() };
                incoming = next.id();
                current = other(&next, current)?;
                if current == side || chain.iter().any(|e: &mt::Edge| e.id() == incoming) {
                    return Err(unsupported("cyclic expanded transition fan"));
                }
                chain.push(next);
                if current == stop {
                    break;
                }
            }
            if current != stop {
                return Err(unsupported("transition fan expansion limit"));
            }
            if !at_start {
                chain.reverse();
            }
            removed.push(edge.id());
            edges.splice(index..=index, chain);
            if edges.len() > 16 {
                return Err(unsupported("too many transition endpoint faces"));
            }
            changed = true;
        }
        if !changed {
            let mut current = fa;
            let mut fan = Vec::new();
            for edge in edges {
                current = other(&edge, current)?;
                fan.push((edge, current));
            }
            if current != fb {
                return Err(unsupported("transition fan does not reach its side face"));
            }
            return Ok((fan, removed));
        }
    }
    Err(unsupported("transition fan expansion exceeded its budget"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concave_boundary_is_exact_bounded_and_independent_of_traversal() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 4.0).unwrap();
        let face =
            body.solid.face_iter().find(|f| matches!(crate::offset::surf_of(f, 1e-7), Some(crate::offset::Surf::Cylinder { .. }))).unwrap().inverse();
        let start = Vec3::new(2.0, 0.0, 1.0);
        let end = Vec3::new(0.0, 2.0, 3.0);
        let (forward, _) = cylinder_boundary(&face, start, end, 1e-7, true, false).unwrap();
        let (reverse, _) = cylinder_boundary(&face, end, start, 1e-7, false, true).unwrap();
        for i in 0..=256 {
            let t = i as f64 / 256.0;
            let p = from_p3(forward.subs(t));
            assert!((p.x.hypot(p.y) - 2.0).abs() < 1e-12);
            assert!((1.0..=3.0).contains(&p.z));
            assert!(p.dist(from_p3(reverse.subs(1.0 - t))) < 1e-12);
        }
        for t in [0.0, 1.0] {
            let tangent = forward.der(t);
            assert!(tangent.x.hypot(tangent.y) < 1e-12);
            assert!(tangent.z > 0.0);
        }
    }

    #[test]
    fn cylindrical_boundary_stays_exact_and_meets_contact_rails_axially() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 4.0).unwrap();
        let face = body.solid.face_iter().find(|f| matches!(crate::offset::surf_of(f, 1e-7), Some(crate::offset::Surf::Cylinder { .. }))).unwrap();
        let start = Vec3::new(2.0, 0.0, 1.0);
        let end = Vec3::new(0.0, 2.0, 3.0);
        let (curve, _) = cylinder_boundary(face, start, end, 1e-7, true, false).unwrap();
        assert!(from_p3(curve.subs(0.0)).dist(start) < 1e-12);
        assert!(from_p3(curve.subs(1.0)).dist(end) < 1e-12);
        for i in 0..=256 {
            let p = from_p3(curve.subs(i as f64 / 256.0));
            assert!((p.x.hypot(p.y) - 2.0).abs() < 1e-12);
            assert!(p.z >= 1.0 && p.z <= 3.0);
        }
        for t in [0.0, 1.0] {
            let d = curve.der(t);
            assert!(d.x.hypot(d.y) < 1e-12);
            assert!(d.z > 1.0);
        }
    }
}
