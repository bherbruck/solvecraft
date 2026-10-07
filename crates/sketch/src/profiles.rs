//! Closed-profile detection.
//!
//! Non-construction lines and arcs form a planar graph (end points within a small tolerance are
//! merged). Dangling edges are pruned, then faces are traced with the "first clockwise turn"
//! rule, which yields every bounded region counter-clockwise. Circles are loops of their own.
//! A loop that lies inside another loop of a different connected component becomes a hole of
//! the innermost loop containing it, so a circle inside a rectangle gives two profiles: the
//! rectangle with a hole, and the disc.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use solvecraft_geom::{Loop2, Region2, Seg2, Vec2};

use crate::model::{CurveKind, Sketch};

const MERGE_TOL: f64 = 1e-6;
const MAX_EDGES: usize = 50_000;

/// A closed region usable by features, with the ids of the curves bounding it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub region: Region2,
    /// Curve ids of the outer loop.
    pub outer_curves: Vec<String>,
    /// Curve ids of each hole loop.
    pub hole_curves: Vec<Vec<String>>,
    pub area: f64,
    pub centroid: Vec2,
}

impl Profile {
    /// All curve ids bounding the profile.
    pub fn curve_ids(&self) -> Vec<String> {
        let mut v = self.outer_curves.clone();
        for h in &self.hole_curves {
            v.extend(h.iter().cloned());
        }
        v
    }
}

struct Edge {
    seg: Seg2,
    u: usize,
    v: usize,
    id: String,
}

/// Parameter of `p` at an end of the shape (lines: 0 or 1), for intersections at end points.
fn end_param(sh: &Shape, p: Vec2, tol: f64) -> Option<f64> {
    match *sh {
        Shape::Line { a, b } => {
            if p.dist(a) <= tol {
                Some(0.0)
            } else if p.dist(b) <= tol {
                Some(1.0)
            } else {
                None
            }
        }
        Shape::Round { c, r, start, sweep } => {
            let s0 = c + Vec2::from_angle(start) * r;
            let s1 = c + Vec2::from_angle(start + sweep) * r;
            (p.dist(s0) <= tol || p.dist(s1) <= tol).then_some(0.0)
        }
    }
}

fn find(p: &mut [usize], mut i: usize) -> usize {
    while let Some(&q) = p.get(i) {
        if q == i {
            break;
        }
        i = q;
    }
    i
}

/// A curve of the sketch as an analytic shape for splitting.
#[derive(Clone, Copy)]
enum Shape {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Circle (full when `sweep` is 2π) from `start` angle sweeping counter-clockwise.
    Round {
        c: Vec2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

impl Shape {
    /// Curve parameter of `p` if it lies on the curve's interior (line t in (0,1); round:
    /// angle offset from start in (0, sweep)).
    fn param_on(&self, p: Vec2, tol: f64) -> Option<f64> {
        match *self {
            Shape::Line { a, b } => {
                let d = b - a;
                let l2 = d.len2();
                if l2 < 1e-24 {
                    return None;
                }
                let t = (p - a).dot(d) / l2;
                let q = a + d * t;
                (q.dist(p) <= tol && t * l2.sqrt() > tol && (1.0 - t) * l2.sqrt() > tol).then_some(t)
            }
            Shape::Round { c, r, start, sweep } => {
                if (p.dist(c) - r).abs() > tol {
                    return None;
                }
                let off = ((p - c).angle() - start).rem_euclid(TAU);
                let ang_tol = tol / r.max(1e-12);
                let full = sweep >= TAU - 1e-12;
                if full {
                    return Some(off);
                }
                (off > ang_tol && off < sweep - ang_tol).then_some(off)
            }
        }
    }
    fn point(&self, t: f64) -> Vec2 {
        match *self {
            Shape::Line { a, b } => a.lerp(b, t),
            Shape::Round { c, r, start, .. } => c + Vec2::from_angle(start + t) * r,
        }
    }
}

/// Intersection points of two shapes (as circles/infinite lines; callers filter by range).
fn intersections(x: &Shape, y: &Shape) -> Vec<Vec2> {
    match (*x, *y) {
        (Shape::Line { a, b }, Shape::Line { a: c, b: d }) => {
            let (r, s) = (b - a, d - c);
            let den = r.cross(s);
            if den.abs() < 1e-12 * r.len() * s.len() {
                return Vec::new();
            }
            let t = (c - a).cross(s) / den;
            vec![a + r * t]
        }
        (Shape::Line { a, b }, Shape::Round { c, r, .. }) | (Shape::Round { c, r, .. }, Shape::Line { a, b }) => {
            let d = b - a;
            let f = a - c;
            let (qa, qb, qc) = (d.len2(), 2.0 * f.dot(d), f.len2() - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa < 1e-24 || disc < 0.0 {
                return Vec::new();
            }
            let sq = disc.sqrt();
            vec![a + d * ((-qb - sq) / (2.0 * qa)), a + d * ((-qb + sq) / (2.0 * qa))]
        }
        (Shape::Round { c: c1, r: r1, .. }, Shape::Round { c: c2, r: r2, .. }) => {
            let dv = c2 - c1;
            let d = dv.len();
            if d < 1e-12 || d > r1 + r2 || d < (r1 - r2).abs() {
                return Vec::new();
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let m = c1 + dv * (a / d);
            let n = dv.perp() / d;
            vec![m + n * h, m - n * h]
        }
    }
}

/// Signed curvature at the start of a segment (left turns positive).
fn curvature(s: &Seg2) -> f64 {
    match *s {
        Seg2::Line { .. } => 0.0,
        Seg2::Arc { radius, sweep, .. } => sweep.signum() / radius.max(1e-12),
    }
}

/// Find all closed profiles of a sketch. Curves are split where other curves end on them or
/// cross them, like a drawing would be read.
pub fn find_profiles(sk: &Sketch) -> Vec<Profile> {
    let tol = MERGE_TOL * 10.0;
    // Analytic shapes of the profile curves.
    let mut shapes: Vec<(usize, Shape)> = Vec::new();
    for (ci, c) in sk.curves.iter().enumerate() {
        if c.construction {
            continue;
        }
        match c.kind {
            CurveKind::Line { a, b } => {
                if let (Some(a), Some(b)) = (sk.point(a), sk.point(b))
                    && a.dist(b) > MERGE_TOL
                {
                    shapes.push((ci, Shape::Line { a, b }));
                }
            }
            CurveKind::Circle { c, r } => {
                if let Some(c) = sk.point(c) {
                    shapes.push((ci, Shape::Round { c, r, start: 0.0, sweep: TAU }));
                }
            }
            CurveKind::Arc { .. } => {
                if let Some(Seg2::Arc { center, radius, start, sweep }) = sk.segs(ci).into_iter().next() {
                    shapes.push((ci, Shape::Round { c: center, r: radius, start, sweep }));
                }
            }
        }
        if shapes.len() > MAX_EDGES {
            break;
        }
    }
    // Split parameters per shape: end points of other curves on it, and crossings.
    let mut ends: Vec<Vec2> = Vec::new();
    for (_, sh) in &shapes {
        match *sh {
            Shape::Line { a, b } => ends.extend([a, b]),
            Shape::Round { sweep, .. } if sweep < TAU - 1e-12 => ends.extend([sh.point(0.0), sh.point(sweep)]),
            Shape::Round { .. } => {}
        }
    }
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); shapes.len()];
    for (i, (_, sh)) in shapes.iter().enumerate() {
        for p in &ends {
            if let Some(t) = sh.param_on(*p, tol)
                && let Some(c) = cuts.get_mut(i)
            {
                c.push(t);
            }
        }
    }
    if shapes.len() <= 2000 {
        for i in 0..shapes.len() {
            for j in (i + 1)..shapes.len() {
                let (Some((_, x)), Some((_, y))) = (shapes.get(i), shapes.get(j)) else { continue };
                let mut pts = intersections(x, y);
                // A tangent touch gives two nearly equal roots: one point.
                if let [p, q] = pts[..]
                    && p.dist(q) < tol * 100.0
                {
                    pts = vec![(p + q) * 0.5];
                }
                for p in pts {
                    // Crossings at (or next to) curve ends are already cuts.
                    if ends.iter().any(|e| e.dist(p) < tol * 100.0) {
                        continue;
                    }
                    let on_x = x.param_on(p, tol).or_else(|| end_param(x, p, tol));
                    let on_y = y.param_on(p, tol).or_else(|| end_param(y, p, tol));
                    if let (Some(tx), Some(ty)) = (on_x, on_y) {
                        if x.param_on(p, tol).is_some()
                            && let Some(c) = cuts.get_mut(i)
                        {
                            c.push(tx);
                        }
                        if y.param_on(p, tol).is_some()
                            && let Some(c) = cuts.get_mut(j)
                        {
                            c.push(ty);
                        }
                    }
                }
            }
        }
    }
    // Pieces → edges between merged nodes.
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node = |p: Vec2| -> usize {
        if let Some(i) = nodes.iter().position(|q| q.dist(p) <= tol) {
            return i;
        }
        nodes.push(p);
        nodes.len() - 1
    };
    let mut loops: Vec<(Loop2, Vec<String>, usize)> = Vec::new(); // (ccw loop, curve ids, component)
    let mut edges: Vec<Edge> = Vec::new();
    for (k, (ci, sh)) in shapes.iter().enumerate() {
        let mut ts = cuts.get(k).cloned().unwrap_or_default();
        ts.sort_by(f64::total_cmp);
        // Cuts closer than a few tolerances are one cut (tangent touches give near-double roots).
        let scale = match *sh {
            Shape::Line { a, b } => a.dist(b),
            Shape::Round { r, .. } => r,
        };
        let close = (tol * 100.0 / scale.max(1e-12)).max(1e-9);
        ts.dedup_by(|a, b| (*a - *b).abs() < close);
        if let (Shape::Round { sweep, .. }, Some(&first), Some(&last)) = (*sh, ts.first(), ts.last())
            && sweep >= TAU - 1e-12
            && ts.len() > 1
            && (first + TAU - last).abs() < close
        {
            ts.pop();
        }
        let id = sk.curves.get(*ci).map(|c| c.id.clone()).unwrap_or_default();
        match *sh {
            Shape::Line { a, b } => {
                let mut ps = vec![a];
                ps.extend(ts.iter().map(|t| sh.point(*t)));
                ps.push(b);
                for w in ps.windows(2) {
                    let (p, q) = (w[0], w[1]);
                    if p.dist(q) > MERGE_TOL {
                        let (u, v) = (node(p), node(q));
                        edges.push(Edge { seg: Seg2::Line { a: p, b: q }, u, v, id: id.clone() });
                    }
                }
            }
            Shape::Round { c, r, start, sweep } => {
                let full = sweep >= TAU - 1e-12;
                if full && ts.is_empty() {
                    loops.push((Loop2::circle(c, r), vec![id.clone()], usize::MAX - *ci));
                    continue;
                }
                let mut bounds: Vec<f64> =
                    if full { ts.clone() } else { std::iter::once(0.0).chain(ts.iter().copied()).chain(std::iter::once(sweep)).collect() };
                if full {
                    let first = bounds.first().copied().unwrap_or(0.0);
                    bounds.push(first + TAU);
                }
                for w in bounds.windows(2) {
                    let (t0, t1) = (w[0], w[1]);
                    if (t1 - t0) * r <= MERGE_TOL {
                        continue;
                    }
                    let seg = Seg2::Arc { center: c, radius: r, start: start + t0, sweep: t1 - t0 };
                    let (u, v) = (node(seg.start()), node(seg.end()));
                    edges.push(Edge { seg, u, v, id: id.clone() });
                }
            }
        }
    }
    let n = nodes.len();
    // Prune dangling edges.
    let mut alive = vec![true; edges.len()];
    loop {
        let mut deg = vec![0usize; n];
        for (e, a) in edges.iter().zip(&alive) {
            if *a {
                if let Some(d) = deg.get_mut(e.u) {
                    *d += 1;
                }
                if let Some(d) = deg.get_mut(e.v) {
                    *d += 1;
                }
            }
        }
        let mut changed = false;
        for (e, a) in edges.iter().zip(alive.iter_mut()) {
            if *a && (deg.get(e.u).copied().unwrap_or(0) < 2 || deg.get(e.v).copied().unwrap_or(0) < 2) {
                *a = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Components over nodes for nesting.
    let mut comp: Vec<usize> = (0..n).collect();
    for (e, a) in edges.iter().zip(&alive) {
        if *a {
            let (ru, rv) = (find(&mut comp, e.u), find(&mut comp, e.v));
            if ru != rv
                && let Some(s) = comp.get_mut(rv)
            {
                *s = ru;
            }
        }
    }

    // Half-edges: 2e = forward (u→v), 2e+1 = backward.
    let half = |h: usize| -> Option<(Seg2, usize, usize)> {
        let e = edges.get(h / 2)?;
        Some(if h.is_multiple_of(2) { (e.seg, e.u, e.v) } else { (e.seg.reversed(), e.v, e.u) })
    };
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (ei, a) in alive.iter().enumerate() {
        if !*a {
            continue;
        }
        for h in [2 * ei, 2 * ei + 1] {
            if let Some((_, from, _)) = half(h)
                && let Some(o) = outgoing.get_mut(from)
            {
                o.push(h);
            }
        }
    }
    let mut visited = vec![false; edges.len() * 2];
    for start in 0..edges.len() * 2 {
        if !alive.get(start / 2).copied().unwrap_or(false) || visited.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut segs = Vec::new();
        let mut ids = Vec::new();
        let mut h = start;
        let mut ok = true;
        for _ in 0..=edges.len() * 2 {
            if let Some(v) = visited.get_mut(h) {
                *v = true;
            }
            let Some((seg, _, to)) = half(h) else {
                ok = false;
                break;
            };
            segs.push(seg);
            if let Some(e) = edges.get(h / 2) {
                ids.push(e.id.clone());
            }
            let back = (-seg.end_tangent()).angle();
            let twin = h ^ 1;
            // First clockwise turn from the way back. Directions are compared a short way
            // along each segment (curvature decides between equal tangents, e.g. a line tangent
            // to an arc, or an arc leaving back along the incoming line).
            let kt = half(twin).map(|(ts, _, _)| curvature(&ts)).unwrap_or(0.0);
            let mut best: Option<(f64, f64, usize)> = None;
            for &o in outgoing.get(to).map(Vec::as_slice).unwrap_or(&[]) {
                if o == twin {
                    continue;
                }
                let Some((os, _, _)) = half(o) else { continue };
                let mut raw = (back - os.start_tangent().angle()).rem_euclid(TAU);
                if raw > TAU - 1e-9 {
                    raw = 0.0;
                }
                let mut d = raw + (kt - curvature(&os)) * 1e-7;
                if d <= 1e-12 {
                    d += TAU;
                }
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, 0.0, o));
                }
            }
            let best = best.map(|(d, _, o)| (d, o));
            let Some((_, next)) = best else {
                ok = false;
                break;
            };
            h = next;
            if h == start {
                break;
            }
            if visited.get(h).copied().unwrap_or(true) {
                ok = false;
                break;
            }
        }
        if !ok || h != start {
            continue;
        }
        let lp = Loop2 { segs };
        if lp.signed_area() > 1e-9 {
            let c = edges.get(start / 2).map(|e| find(&mut comp, e.u)).unwrap_or(0);
            loops.push((lp, ids, c));
        }
    }

    // Nesting: parent = smallest containing loop from another component.
    let areas: Vec<f64> = loops.iter().map(|(l, _, _)| l.signed_area().abs()).collect();
    let mut parent_of: Vec<Option<usize>> = vec![None; loops.len()];
    for (i, (li, _, ci)) in loops.iter().enumerate() {
        let probe = li.segs.first().map(Seg2::mid).unwrap_or_default();
        let ai = areas.get(i).copied().unwrap_or(0.0);
        let mut best: Option<(f64, usize)> = None;
        for (j, (lj, _, cj)) in loops.iter().enumerate() {
            let aj = areas.get(j).copied().unwrap_or(0.0);
            if i == j || ci == cj || aj <= ai {
                continue;
            }
            if lj.contains(probe) && best.is_none_or(|(ba, _)| aj < ba) {
                best = Some((aj, j));
            }
        }
        if let Some(p) = parent_of.get_mut(i) {
            *p = best.map(|(_, j)| j);
        }
    }
    let mut out: Vec<Profile> = loops
        .iter()
        .enumerate()
        .map(|(i, (l, ids, _))| {
            let kids: Vec<usize> = (0..loops.len()).filter(|k| parent_of.get(*k).copied().flatten() == Some(i)).collect();
            let holes: Vec<Loop2> = kids.iter().filter_map(|k| loops.get(*k).map(|(h, _, _)| h.ccw().reversed())).collect();
            let hole_curves: Vec<Vec<String>> = kids.iter().filter_map(|k| loops.get(*k).map(|(_, ids, _)| ids.clone())).collect();
            let region = Region2 { outer: l.ccw(), holes };
            let area = region.area();
            let centroid = region.centroid();
            Profile { region, outer_curves: ids.clone(), hole_curves, area, centroid }
        })
        .collect();
    let first_curve = |p: &Profile| p.outer_curves.iter().filter_map(|id| sk.curve_index(id)).min().unwrap_or(usize::MAX);
    out.sort_by(|a, b| first_curve(a).cmp(&first_curve(b)).then(b.area.total_cmp(&a.area)));
    out
}

fn same_seg(a: &Seg2, b: &Seg2, tol: f64) -> bool {
    match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: r, b: s }) => p.dist(r) <= tol && q.dist(s) <= tol,
        (Seg2::Arc { center: c1, radius: r1, .. }, Seg2::Arc { center: c2, radius: r2, .. }) => {
            c1.dist(c2) <= tol
                && (r1 - r2).abs() <= tol
                && a.start().dist(b.start()) <= tol
                && a.end().dist(b.end()) <= tol
                && a.mid().dist(b.mid()) <= tol
        }
        _ => false,
    }
}

/// Union of regions that touch along shared boundary segments (one extrude of several adjacent
/// profiles makes one body). Disjoint regions stay separate.
pub fn merge_regions(regions: &[Region2]) -> Vec<Region2> {
    let tol = MERGE_TOL * 10.0;
    // Every boundary segment oriented with the region on its left.
    let mut segs: Vec<Seg2> = Vec::new();
    for r in regions {
        segs.extend(r.outer.ccw().segs);
        for h in &r.holes {
            segs.extend(h.ccw().reversed().segs);
        }
    }
    if segs.len() > MAX_EDGES {
        return regions.to_vec();
    }
    // Drop pairs that run against each other (shared boundaries).
    let mut alive = vec![true; segs.len()];
    let mut shared = false;
    for i in 0..segs.len() {
        if !alive.get(i).copied().unwrap_or(false) {
            continue;
        }
        for j in (i + 1)..segs.len() {
            if alive.get(j).copied().unwrap_or(false)
                && let (Some(a), Some(b)) = (segs.get(i), segs.get(j))
                && same_seg(a, &b.reversed(), tol)
            {
                alive[i] = false;
                alive[j] = false;
                shared = true;
                break;
            }
        }
    }
    if !shared {
        return regions.to_vec();
    }
    let rest: Vec<Seg2> = segs.iter().zip(&alive).filter(|(_, a)| **a).map(|(s, _)| *s).collect();
    // Chain into loops, taking the sharpest left turn where several continue.
    let mut used = vec![false; rest.len()];
    let mut loops: Vec<Loop2> = Vec::new();
    for start in 0..rest.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = Vec::new();
        let mut cur = start;
        for _ in 0..=rest.len() {
            if let Some(u) = used.get_mut(cur) {
                *u = true;
            }
            let Some(seg) = rest.get(cur).copied() else { break };
            lp.push(seg);
            let end = seg.end();
            if let Some(first) = rest.get(start)
                && end.dist(first.start()) <= tol
                && lp.len() > 1
            {
                break;
            }
            let back = (-seg.end_tangent()).angle();
            let next = (0..rest.len())
                .filter(|k| !used.get(*k).copied().unwrap_or(true) && rest.get(*k).is_some_and(|s| s.start().dist(end) <= tol))
                .min_by(|x, y| {
                    let d = |k: usize| rest.get(k).map(|s| (back - s.start_tangent().angle()).rem_euclid(TAU)).unwrap_or(TAU);
                    d(*x).total_cmp(&d(*y))
                });
            match next {
                Some(k) => cur = k,
                None => break,
            }
        }
        if !lp.is_empty() && lp.first().zip(lp.last()).is_some_and(|(f, l)| l.end().dist(f.start()) <= tol) {
            loops.push(Loop2 { segs: lp });
        }
    }
    let outers: Vec<&Loop2> = loops.iter().filter(|l| l.signed_area() > 0.0).collect();
    let holes: Vec<&Loop2> = loops.iter().filter(|l| l.signed_area() < 0.0).collect();
    let mut out: Vec<Region2> = outers.iter().map(|o| Region2 { outer: (*o).clone(), holes: Vec::new() }).collect();
    for h in holes {
        let probe = h.segs.first().map(Seg2::mid).unwrap_or_default();
        // Innermost containing outer loop.
        let best = out.iter_mut().filter(|r| r.outer.contains(probe)).min_by(|a, b| a.outer.signed_area().total_cmp(&b.outer.signed_area()));
        if let Some(r) = best {
            r.holes.push(h.clone());
        }
    }
    if out.is_empty() { regions.to_vec() } else { out }
}
