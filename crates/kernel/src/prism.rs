//! Booleans of prisms in 2D: when the target is a prism (two caps square to one direction,
//! walls of planes and cylinders along it) and the tool a prism along the same direction
//! spanning the target's height, the result is the prism over the 2D boolean of their
//! sections. Plates with holes, slots and patterns of them: exact, and immune to the mesh
//! intersection's trouble with walls meeting along lines.

use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3};
use crate::{BoolOp, Result};

/// A prism: the plane of its bottom cap (normal along the walls), its height and section.
struct Prism {
    lo: f64,
    hi: f64,
    regions: Vec<Region2>,
}

/// Edges of a cap face as 2D segments (lines and arcs only).
fn seg2(e: &mt::Edge, plane: &Plane, tol: f64) -> Option<Vec<Seg2>> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec2> = (0..=16).map(|k| plane.to_local(from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0)))).collect();
    let (a, b) = (*pts.first()?, *pts.last()?);
    let closed = a.dist(b) < tol;
    let seg_dist = |p: Vec2| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.dot(ab).max(1e-300)).clamp(0.0, 1.0);
        (a + ab * t).dist(p)
    };
    if !closed && pts.iter().all(|p| seg_dist(*p) < tol) {
        return Some(vec![Seg2::Line { a, b }]);
    }
    // Circle through three samples.
    let (p, q, r) = (a, *pts.get(5)?, *pts.get(11)?);
    let d = 2.0 * (p.x * (q.y - r.y) + q.x * (r.y - p.y) + r.x * (p.y - q.y));
    if d.abs() < 1e-300 {
        return None;
    }
    let sq = |v: Vec2| v.x * v.x + v.y * v.y;
    let cx = (sq(p) * (q.y - r.y) + sq(q) * (r.y - p.y) + sq(r) * (p.y - q.y)) / d;
    let cy = (sq(p) * (r.x - q.x) + sq(q) * (p.x - r.x) + sq(r) * (q.x - p.x)) / d;
    let center = Vec2::new(cx, cy);
    let radius = center.dist(a);
    if !pts.iter().all(|x| (x.dist(center) - radius).abs() < tol) {
        return None;
    }
    let ang = |v: Vec2| (v - center).angle();
    let start = ang(a);
    // Signed sweep by unwrapping the samples.
    let mut sweep = 0.0;
    let mut prev = start;
    for x in pts.iter().skip(1) {
        let mut da = ang(*x) - prev;
        while da > std::f64::consts::PI {
            da -= std::f64::consts::TAU;
        }
        while da < -std::f64::consts::PI {
            da += std::f64::consts::TAU;
        }
        sweep += da;
        prev = ang(*x);
    }
    if closed {
        let h = sweep / 2.0;
        return Some(vec![Seg2::Arc { center, radius, start, sweep: h }, Seg2::Arc { center, radius, start: start + h, sweep: h }]);
    }
    Some(vec![Seg2::Arc { center, radius, start, sweep }])
}

/// The body as a prism along `d` (unit), seen in `plane` (normal `d`).
fn prism_of(b: &Body, d: Vec3, plane: &Plane, tol: f64) -> Option<Prism> {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut tops: Vec<&mt::Face> = Vec::new();
    let mut top_h = Vec::new();
    for f in b.solid.face_iter() {
        let surf = f.oriented_surface();
        if let mt::Surface::Plane(p) = &surf {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            let h = plane.height(from_p3(p.origin()));
            if n.dot(d) > 1.0 - 1e-9 {
                tops.push(f);
                top_h.push(h);
                hi = hi.max(h);
                continue;
            }
            if n.dot(d) < -1.0 + 1e-9 {
                lo = lo.min(h);
                continue;
            }
            if n.dot(d).abs() > 1e-9 {
                return None;
            }
            continue;
        }
        // Walls: normals square to d at sampled boundary points.
        for w in f.boundaries() {
            for v in w.vertex_iter() {
                let (u, s) = surf.search_nearest_parameter(v.point(), None, 50)?;
                let n = surf.normal(u, s);
                if Vec3::new(n.x, n.y, n.z).dot(d).abs() > 1e-7 {
                    return None;
                }
            }
        }
    }
    // One height for every top (and bottom) cap.
    if top_h.iter().any(|h| (h - hi).abs() > tol) || !(lo.is_finite() && hi > lo + tol) {
        return None;
    }
    let mut regions = Vec::new();
    for f in tops {
        let mut loops = Vec::new();
        for w in f.boundaries() {
            let mut segs = Vec::new();
            for e in w.edge_iter() {
                segs.extend(seg2(e, plane, tol)?);
            }
            loops.push(Loop2 { segs });
        }
        let (outer, holes) = split_outer(loops)?;
        regions.push(Region2 { outer, holes });
    }
    Some(Prism { lo, hi, regions })
}

/// The loop with the largest area is the outer one.
fn split_outer(mut loops: Vec<Loop2>) -> Option<(Loop2, Vec<Loop2>)> {
    let i = (0..loops.len()).max_by(|a, b| loops[*a].signed_area().abs().total_cmp(&loops[*b].signed_area().abs()))?;
    let outer = loops.remove(i);
    Some((outer, loops))
}

fn heights_of(b: &Body, plane: &Plane) -> (f64, f64) {
    b.solid.vertex_iter().map(|v| plane.height(from_p3(v.point()))).fold((f64::INFINITY, f64::NEG_INFINITY), |m, h| (m.0.min(h), m.1.max(h)))
}

/// Points where two segments cross (parameters on each), lines and arcs only.
fn crossings(s: &Seg2, t: &Seg2, tol: f64) -> Vec<(f64, f64)> {
    let mut pts: Vec<Vec2> = Vec::new();
    match (*s, *t) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            let (r, q) = (b - a, d - c);
            let den = r.cross(q);
            if den.abs() > 1e-300 {
                let u = (c - a).cross(q) / den;
                pts.push(a + r * u);
            }
        }
        (Seg2::Line { a, b }, Seg2::Arc { center, radius, .. }) | (Seg2::Arc { center, radius, .. }, Seg2::Line { a, b }) => {
            let dir = b - a;
            let f = a - center;
            let (qa, qb, qc) = (dir.dot(dir), 2.0 * f.dot(dir), f.dot(f) - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 1e-300 && disc >= -tol * tol * qa {
                let sd = disc.max(0.0).sqrt();
                for u in [(-qb - sd) / (2.0 * qa), (-qb + sd) / (2.0 * qa)] {
                    pts.push(a + dir * u);
                }
            }
        }
        (Seg2::Arc { center: c0, radius: r0, .. }, Seg2::Arc { center: c1, radius: r1, .. }) => {
            let dd = c0.dist(c1);
            if dd > 1e-12 && dd <= r0 + r1 + tol && dd >= (r0 - r1).abs() - tol {
                let a = (r0 * r0 - r1 * r1 + dd * dd) / (2.0 * dd);
                let h = (r0 * r0 - a * a).max(0.0).sqrt();
                let m = c0 + (c1 - c0) * (a / dd);
                let perp = (c1 - c0).perp() * (1.0 / dd);
                pts.push(m + perp * h);
                pts.push(m - perp * h);
            }
        }
        _ => {}
    }
    let mut out = Vec::new();
    for p in pts {
        let (u, v) = (s.closest_param(p), t.closest_param(p));
        if s.point_at(u).dist(p) < tol && t.point_at(v).dist(p) < tol {
            out.push((u, v));
        }
    }
    out
}

/// Exact point-in-loop by winding (arcs as fine polylines are enough away from the boundary).
fn inside(regions: &[Region2], p: Vec2) -> bool {
    regions.iter().any(|r| r.contains(p))
}

fn dist_to(regions: &[Region2], p: Vec2) -> f64 {
    regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| l.segs.iter())
        .map(|s| s.point_at(s.closest_param(p)).dist(p))
        .fold(f64::INFINITY, f64::min)
}

/// Direction of the boundary of `regions` through `p` (on it), as the boundary runs.
fn dir_at(regions: &[Region2], p: Vec2, tol: f64) -> Option<Vec2> {
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            for s in &l.segs {
                let t = s.closest_param(p);
                if s.point_at(t).dist(p) < tol {
                    return s.derivative(t).normalized();
                }
            }
        }
    }
    None
}

/// Loops oriented: outers counter-clockwise, holes clockwise.
fn oriented(regions: &[Region2]) -> Vec<Region2> {
    regions
        .iter()
        .map(|r| Region2 {
            outer: if r.outer.signed_area() < 0.0 { r.outer.reversed() } else { r.outer.clone() },
            holes: r.holes.iter().map(|h| if h.signed_area() > 0.0 { h.reversed() } else { h.clone() }).collect(),
        })
        .collect()
}

/// 2D boolean of regions bounded by lines and arcs.
fn boolean2d(a: &[Region2], b: &[Region2], op: BoolOp, tol: f64) -> Option<Vec<Region2>> {
    let (a, b) = (oriented(a), oriented(b));
    let segs =
        |rs: &[Region2]| -> Vec<Seg2> { rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| l.segs.clone()).collect() };
    let (sa, sb) = (segs(&a), segs(&b));
    if sa.len() * sb.len() > 4_000_000 || sa.iter().chain(&sb).any(|s| !matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })) {
        return None;
    }
    // Split every segment where it crosses the other side's boundary.
    let mut cut_a: Vec<Vec<f64>> = vec![Vec::new(); sa.len()];
    let mut cut_b: Vec<Vec<f64>> = vec![Vec::new(); sb.len()];
    for (i, s) in sa.iter().enumerate() {
        for (j, t) in sb.iter().enumerate() {
            for (u, v) in crossings(s, t, tol) {
                cut_a.get_mut(i)?.push(u);
                cut_b.get_mut(j)?.push(v);
            }
        }
    }
    // Ends of the other side's segments lying on a segment split it too (touching corners).
    for (i, s) in sa.iter().enumerate() {
        for t in &sb {
            for p in [t.start(), t.end()] {
                let u = s.closest_param(p);
                if s.point_at(u).dist(p) < tol {
                    cut_a.get_mut(i)?.push(u);
                }
            }
        }
    }
    for (j, t) in sb.iter().enumerate() {
        for s in &sa {
            for p in [s.start(), s.end()] {
                let v = t.closest_param(p);
                if t.point_at(v).dist(p) < tol {
                    cut_b.get_mut(j)?.push(v);
                }
            }
        }
    }
    let pieces = |s: &Seg2, cuts: &mut Vec<f64>| -> Vec<Seg2> {
        cuts.sort_by(f64::total_cmp);
        let mut out = Vec::new();
        let mut rest = *s;
        let mut done = 0.0;
        for &c in cuts.iter() {
            if c <= done + 1e-9 || c >= 1.0 - 1e-9 {
                continue;
            }
            let local = (c - done) / (1.0 - done);
            let (x, y) = rest.split_at(local);
            if x.length() > tol {
                out.push(x);
                rest = y;
                done = c;
            }
        }
        out.push(rest);
        out
    };
    let mut kept: Vec<Seg2> = Vec::new();
    // A's pieces.
    for (i, s) in sa.iter().enumerate() {
        for p in pieces(s, cut_a.get_mut(i)?) {
            let m = p.point_at(0.5);
            let on = dist_to(&b, m) < tol;
            let keep = if on {
                let same = dir_at(&b, m, tol).is_some_and(|d| d.dot(p.derivative(0.5)) > 0.0);
                match op {
                    BoolOp::Union | BoolOp::Intersect => same,
                    BoolOp::Cut => !same,
                }
            } else {
                let ins = inside(&b, m);
                match op {
                    BoolOp::Union | BoolOp::Cut => !ins,
                    BoolOp::Intersect => ins,
                }
            };
            if keep {
                kept.push(p);
            }
        }
    }
    // B's pieces (coincident ones were decided with A's).
    for (j, t) in sb.iter().enumerate() {
        for p in pieces(t, cut_b.get_mut(j)?) {
            let m = p.point_at(0.5);
            if dist_to(&a, m) < tol {
                continue;
            }
            let ins = inside(&a, m);
            match op {
                BoolOp::Union if !ins => kept.push(p),
                BoolOp::Intersect if ins => kept.push(p),
                BoolOp::Cut if ins => kept.push(p.reversed()),
                _ => {}
            }
        }
    }
    // Chain into loops.
    let mut loops: Vec<Loop2> = Vec::new();
    let mut used = vec![false; kept.len()];
    for start in 0..kept.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut segs = vec![*kept.get(start)?];
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let first = kept.get(start)?.start();
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > kept.len() + 2 {
                return None;
            }
            let end = segs.last()?.end();
            if end.dist(first) < tol * 10.0 && segs.len() > 1 {
                break;
            }
            let next = (0..kept.len())
                .filter(|k| !used.get(*k).copied().unwrap_or(true))
                .min_by(|x, y| kept[*x].start().dist(end).total_cmp(&kept[*y].start().dist(end)))?;
            if kept.get(next)?.start().dist(end) > tol * 10.0 {
                if end.dist(first) < tol * 10.0 {
                    break;
                }
                return None;
            }
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            segs.push(*kept.get(next)?);
        }
        let l = Loop2 { segs };
        if l.signed_area().abs() > tol * tol {
            loops.push(l);
        }
    }
    // Outers (counter-clockwise) take the holes they contain, the smallest first.
    let (mut outers, holes): (Vec<Loop2>, Vec<Loop2>) = loops.into_iter().partition(|l| l.signed_area() > 0.0);
    outers.sort_by(|x, y| x.signed_area().abs().total_cmp(&y.signed_area().abs()));
    let mut regions: Vec<Region2> = outers.into_iter().map(|o| Region2 { outer: o, holes: Vec::new() }).collect();
    for h in holes {
        let p = h.segs.first()?.point_at(0.5);
        let r = regions.iter_mut().find(|r| r.outer.contains(p) && r.outer.signed_area().abs() > h.signed_area().abs())?;
        r.holes.push(h);
    }
    Some(regions)
}

/// The boolean done on prism sections, or `None` when it doesn't apply.
pub(crate) fn prism_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9) * 10.0;
    // Candidate directions: a's planar face normals (each line once).
    let mut dirs: Vec<Vec3> = Vec::new();
    for f in a.solid.face_iter() {
        if let mt::Surface::Plane(p) = f.oriented_surface() {
            let n = p.normal();
            if let Some(n) = Vec3::new(n.x, n.y, n.z).normalized()
                && !dirs.iter().any(|m| m.dot(n).abs() > 1.0 - 1e-9)
            {
                dirs.push(if n.dot(Vec3::new(1e-3, 1e-6, 1.0)) < 0.0 { -n } else { n });
            }
        }
    }
    let (d, plane, pa, pb) = dirs.into_iter().take(8).find_map(|d| {
        let plane = Plane::from_normal(Vec3::ZERO, d)?;
        let pa = prism_of(a, d, &plane, tol)?;
        let pb = prism_of(b, d, &plane, tol)?;
        Some((d, plane, pa, pb))
    })?;
    let _ = d;
    let (blo, bhi) = heights_of(b, &plane);
    let spans = blo <= pa.lo + tol && bhi >= pa.hi - tol;
    let same = (pb.lo - pa.lo).abs() < tol && (pb.hi - pa.hi).abs() < tol;
    let area =
        |rs: &[Region2]| rs.iter().map(|r| r.outer.signed_area().abs() - r.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()).sum::<f64>();
    // Prisms of one section overlapping or touching end to end: one longer prism.
    let stacked = op == BoolOp::Union && !same && pb.lo <= pa.hi + tol && pa.lo <= pb.hi + tol;
    let ok = match op {
        BoolOp::Cut | BoolOp::Intersect => spans,
        BoolOp::Union => same || stacked,
    };
    if !ok {
        return None;
    }
    let regions = boolean2d(&pa.regions, &pb.regions, op, tol)?;
    if regions.is_empty() {
        return Some(Ok(None));
    }
    let (lo, hi) = if stacked {
        // Only when the sections are the same (the union adds nothing to either).
        let (u, x, y) = (area(&regions), area(&pa.regions), area(&pb.regions));
        if (u - x).abs() > tol * (1.0 + u) || (u - y).abs() > tol * (1.0 + u) {
            return None;
        }
        (pa.lo.min(pb.lo), pa.hi.max(pb.hi))
    } else {
        (pa.lo, pa.hi)
    };
    let base = plane.offset(lo);
    let mut bodies = match crate::extrude(&base, &regions, 0.0, hi - lo) {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    let first = bodies.pop()?;
    if !bodies.is_empty() {
        // Several pieces: one body of several lumps.
        let mut shells = first.solid.boundaries().clone();
        for b in bodies {
            shells.extend(b.solid.boundaries().iter().cloned());
        }
        return Some(crate::body::Solid::try_new(shells).map_err(|e| crate::KernelError::Failed(e.to_string())).and_then(Body::new).map(Some));
    }
    Some(Ok(Some(first)))
}
