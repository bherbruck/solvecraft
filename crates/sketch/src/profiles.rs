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
    curve: usize,
    seg: Seg2,
    u: usize,
    v: usize,
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

/// Find all closed profiles of a sketch.
pub fn find_profiles(sk: &Sketch) -> Vec<Profile> {
    // Node ids: point indices merged by position.
    let n = sk.points.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        let (pa, pb) = (sk.points.get(*a).map(|p| p.pos.x).unwrap_or(0.0), sk.points.get(*b).map(|p| p.pos.x).unwrap_or(0.0));
        pa.total_cmp(&pb)
    });
    for (k, &i) in order.iter().enumerate() {
        let Some(pi) = sk.points.get(i).map(|p| p.pos) else { continue };
        for &j in order.iter().skip(k + 1) {
            let Some(pj) = sk.points.get(j).map(|p| p.pos) else { continue };
            if pj.x - pi.x > MERGE_TOL {
                break;
            }
            if pi.dist(pj) <= MERGE_TOL {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj
                    && let Some(s) = parent.get_mut(rj)
                {
                    *s = ri;
                }
            }
        }
    }

    let mut loops: Vec<(Loop2, Vec<String>, usize)> = Vec::new(); // (ccw loop, curve ids, component)
    let mut edges: Vec<Edge> = Vec::new();
    for (ci, c) in sk.curves.iter().enumerate() {
        if c.construction {
            continue;
        }
        match c.kind {
            CurveKind::Circle { .. } => {
                let segs = sk.segs(ci);
                if !segs.is_empty() {
                    loops.push((Loop2 { segs }, vec![c.id.clone()], usize::MAX - ci));
                }
            }
            CurveKind::Line { a, b } | CurveKind::Arc { a, b, .. } => {
                let (u, v) = (find(&mut parent, a), find(&mut parent, b));
                if u == v {
                    continue;
                }
                if let Some(seg) = sk.segs(ci).into_iter().next()
                    && seg.length() > MERGE_TOL
                {
                    edges.push(Edge { curve: ci, seg, u, v });
                }
            }
        }
        if edges.len() > MAX_EDGES {
            break;
        }
    }

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
            if let Some(e) = edges.get(h / 2)
                && let Some(c) = sk.curves.get(e.curve)
            {
                ids.push(c.id.clone());
            }
            let back = (-seg.end_tangent()).angle();
            let twin = h ^ 1;
            let mut best: Option<(f64, usize)> = None;
            for &o in outgoing.get(to).map(Vec::as_slice).unwrap_or(&[]) {
                if o == twin {
                    continue;
                }
                let Some((os, _, _)) = half(o) else { continue };
                let mut d = (back - os.start_tangent().angle()) % TAU;
                if d <= 1e-12 {
                    d += TAU;
                }
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, o));
                }
            }
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
