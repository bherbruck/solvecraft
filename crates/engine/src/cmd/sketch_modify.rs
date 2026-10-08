//! Sketch modify tools: trim, extend, break, fillet, chamfer, offset, mirror, circular and
//! rectangular patterns, move/copy and scale.

use std::f64::consts::TAU;

use serde_json::{Value, json};
use solvecraft_geom::Vec2;
use solvecraft_sketch::{ConstraintKind, Curve, CurveKind, Shape, Sketch, intersections};

use super::sketch::{add_c, edit, ids_of};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, expr, num, req_vec2, str_, string_list, vec2};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FilletSketchCmd", "Fillet", fillet)
        .at("SKETCH", "MODIFY")
        .icon("sketch_fillet")
        .enabled(in_sketch)
        .params("point: corner shared by two lines (\"l1.end\") | a, b: two lines meeting at a corner; radius: expr"),
    CommandSpec::new("ChamferSketchEqualDistance", "Equal Distance Chamfer", chamfer_equal)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer")
        .enabled(in_sketch)
        .params("point | a, b: a corner of two lines; distance"),
    CommandSpec::new("ChamferSketchDistanceAngle", "Distance and Angle Chamfer", chamfer_dist_angle)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer")
        .enabled(in_sketch)
        .params("point | a, b: a corner of two lines; distance: along a, angle: deg from a"),
    CommandSpec::new("ChamferSketchDistanceDistance", "Two Distance Chamfer", chamfer_two)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer")
        .enabled(in_sketch)
        .params("point | a, b: a corner of two lines; distance: along a, distance2: along b"),
    CommandSpec::new("Offset", "Offset", offset)
        .at("SKETCH", "MODIFY")
        .icon("offset")
        .key("O")
        .enabled(in_sketch)
        .params("curves: [ids], distance: number (positive = left of the chain, or toward `side`), side?: [x,y], chain?: bool (default true: the connected chain)"),
    CommandSpec::new("TrimSketchCmd", "Trim", trim)
        .at("SKETCH", "MODIFY")
        .icon("trim")
        .key("T")
        .enabled(in_sketch)
        .params("curve: id, at: [x,y] (the piece to remove: between the nearest intersections around it)"),
    CommandSpec::new("ExtendSketchCmd", "Extend", extend)
        .at("SKETCH", "MODIFY")
        .icon("extend")
        .enabled(in_sketch)
        .params("curve: id, at: [x,y] near the end to extend (to the nearest curve)"),
    CommandSpec::new("BreakSketchCmd", "Break", break_cmd)
        .at("SKETCH", "MODIFY")
        .icon("break")
        .enabled(in_sketch)
        .params("curve: id, at: [x,y] (split at the nearest intersections around it) | params: split at points: [[x,y]…]"),
    CommandSpec::new("SketchScaleCmd", "Sketch Scale", scale)
        .at("SKETCH", "MODIFY")
        .icon("scale")
        .enabled(in_sketch)
        .params("entities: [curve/point ids], base: [x,y] or point ref, factor: number"),
    CommandSpec::new("sketch.move", "Move/Copy Sketch Entities", move_copy)
        .enabled(in_sketch)
        .params("entities: [ids], translate?: [dx,dy], angle?: deg, center?: [x,y], copy?: bool"),
    CommandSpec::new("MirrorSketchCommand", "Mirror", mirror)
        .at("SKETCH", "CREATE")
        .icon("sketch_mirror")
        .enabled(in_sketch)
        .params("entities: [curve/point ids], line: mirror line id (symmetry constraints are added)"),
    CommandSpec::new("CircularSketchPatternCommand", "Circular Pattern", pattern_circular)
        .at("SKETCH", "CREATE")
        .icon("sketch_circ_pattern")
        .enabled(in_sketch)
        .params("entities: [ids], center: [x,y] or point ref, count: 2…500, angle?: total deg (default 360)"),
    CommandSpec::new("RectangularSketchPatternCommand", "Rectangular Pattern", pattern_rect)
        .at("SKETCH", "CREATE")
        .icon("sketch_rect_pattern")
        .enabled(in_sketch)
        .params("entities: [ids], dir?: [x,y] (default x), count, spacing; count2?, spacing2? (perpendicular direction)"),
];

const TOL: f64 = 1e-7;
const MAX_COPIES: usize = 500;

fn curve_arg(sk: &Sketch, p: &Value, cmd: &str) -> Result<usize> {
    let id = str_(p, "curve").ok_or_else(|| bad(cmd, "`curve` must be a curve id"))?;
    let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
    if sk.is_linked_curve(c) {
        return Err(bad(cmd, format!("`{id}` is projected geometry: break its link first")));
    }
    Ok(c)
}

/// Parameter of `p` on the bounded shape, ends included.
fn on_bounded(sh: &Shape, p: Vec2) -> Option<f64> {
    if sh.dist(p) > TOL * 10.0 {
        return None;
    }
    let t = sh.param(p);
    let (t0, t1) = sh.range();
    let slack = match *sh {
        Shape::Line { a, b } => TOL * 10.0 / a.dist(b).max(1e-12),
        Shape::Round { r, .. } => TOL * 10.0 / r.max(1e-12),
    };
    if sh.is_full() || (t >= t0 - slack && t <= t1 + slack) { Some(t) } else { None }
}

/// Points where other curves meet curve `ci` (crossings and end points touching it), with the
/// curve that makes each one and the parameter on `ci`.
fn cut_points(sk: &Sketch, ci: usize) -> Vec<(f64, Vec2, usize)> {
    let Some(sh) = sk.shape(ci) else { return Vec::new() };
    let mut out: Vec<(f64, Vec2, usize)> = Vec::new();
    for (oi, _) in sk.curves.iter().enumerate() {
        if oi == ci {
            continue;
        }
        let Some(other) = sk.shape(oi) else { continue };
        let mut pts = intersections(&sh, &other);
        // Overlapping or touching ends that the analytic intersection misses (parallel lines).
        if let Shape::Line { a, b } = other {
            pts.extend([a, b]);
        }
        if let Shape::Round { sweep, .. } = other
            && sweep < TAU - 1e-12
        {
            pts.extend([other.point(0.0), other.point(sweep)]);
        }
        for q in pts {
            if on_bounded(&other, q).is_some()
                && let Some(t) = on_bounded(&sh, q)
                && !out.iter().any(|(_, x, _)| x.dist(q) < TOL * 10.0)
            {
                out.push((t, q, oi));
            }
        }
    }
    out
}

/// Constrain a new end point onto the curve that cut there.
fn pin(sk: &mut Sketch, p: usize, cutter: usize) -> Result<()> {
    let at = sk.point(p).unwrap_or_default();
    // An end point of the cutter there: coincident with it; otherwise on the curve.
    let ends: Vec<usize> = match sk.curves.get(cutter).map(|c| c.kind.clone()) {
        Some(CurveKind::Line { a, b }) | Some(CurveKind::Arc { a, b, .. }) => vec![a, b],
        _ => Vec::new(),
    };
    if let Some(e) = ends.into_iter().find(|e| sk.point(*e).is_some_and(|q| q.dist(at) < TOL * 10.0)) {
        if e != p {
            add_c(sk, ConstraintKind::Coincident { p, q: e })?;
        }
        return Ok(());
    }
    add_c(sk, ConstraintKind::PointOnCurve { p, c: cutter })?;
    Ok(())
}

/// Split curve `ci` at interior parameters `ts` (sorted). Returns the pieces in parameter
/// order (the first keeps the original index) and the new points.
fn split_at(sk: &mut Sketch, ci: usize, ts: &[f64]) -> Result<(Vec<usize>, Vec<usize>)> {
    let sh = sk.shape(ci).ok_or_else(|| bad("split", "curve"))?;
    let kind = sk.curves.get(ci).map(|c| c.kind.clone()).ok_or_else(|| bad("split", "curve"))?;
    let construction = sk.curves.get(ci).is_some_and(|c| c.construction);
    let mut pts = Vec::new();
    for t in ts {
        pts.push(sk.add_point(sh.point(*t), None)?);
    }
    let mut pieces = vec![ci];
    let mut push = |sk: &mut Sketch, kind: CurveKind, prefix: &str| -> Result<()> {
        let id = sk.fresh(prefix);
        sk.curves.push(Curve { id, kind, construction, reversed: false, link: None, centerline: false });
        pieces.push(sk.curves.len() - 1);
        Ok(())
    };
    match kind {
        CurveKind::Line { a, b } => {
            let mut chain = vec![a];
            chain.extend(&pts);
            chain.push(b);
            if let Some(c) = sk.curves.get_mut(ci) {
                c.kind = CurveKind::Line { a, b: chain.get(1).copied().unwrap_or(b) };
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Line { a: w[0], b: w[1] }, "l")?;
            }
        }
        CurveKind::Arc { c, a, b } => {
            let mut chain = vec![a];
            chain.extend(&pts);
            chain.push(b);
            if let Some(cu) = sk.curves.get_mut(ci) {
                cu.kind = CurveKind::Arc { c, a, b: chain.get(1).copied().unwrap_or(b) };
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Arc { c, a: w[0], b: w[1] }, "a")?;
            }
        }
        CurveKind::Circle { c, .. } => {
            if pts.len() < 2 {
                return Err(bad("split", "a circle needs two points to split at"));
            }
            let mut chain = pts.clone();
            if let Some(f) = pts.first() {
                chain.push(*f);
            }
            if let Some(cu) = sk.curves.get_mut(ci) {
                cu.kind = CurveKind::Arc { c, a: chain[0], b: chain[1] };
                cu.reversed = false;
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Arc { c, a: w[0], b: w[1] }, "a")?;
            }
        }
        _ => return Err(bad("split", "not supported yet for ellipses, splines and conics")),
    }
    Ok((pieces, pts))
}

/// Keep the pieces of a split tied together: lines collinear, arcs on one circle.
fn tie(sk: &mut Sketch, pieces: &[usize]) -> Result<()> {
    for w in pieces.windows(2) {
        let (a, b) = (w[0], w[1]);
        let line = matches!(sk.curves.get(a).map(|c| &c.kind), Some(CurveKind::Line { .. }));
        add_c(sk, if line { ConstraintKind::Collinear { a, b } } else { ConstraintKind::Equal { a, b } })?;
    }
    Ok(())
}

/// The cut parameters bracketing `tq` on curve `ci`: (before, after) with their cutter curves.
fn bracket(sk: &Sketch, ci: usize, tq: f64) -> (Vec<(f64, Vec2, usize)>, bool) {
    let Some(sh) = sk.shape(ci) else { return (Vec::new(), false) };
    let (t0, t1) = sh.range();
    let full = sh.is_full();
    let span = t1 - t0;
    let mut cuts: Vec<(f64, Vec2, usize)> =
        cut_points(sk, ci).into_iter().filter(|(t, _, _)| full || (*t > t0 + 1e-9 * span && *t < t1 - 1e-9 * span)).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    if full {
        if cuts.len() < 2 {
            return (Vec::new(), true);
        }
        // Cuts around tq on the circle (wrapping).
        let after = cuts.iter().position(|c| c.0 > tq).unwrap_or(0);
        let before = (after + cuts.len() - 1) % cuts.len();
        let mut two = vec![cuts[before], cuts[after]];
        two.sort_by(|a, b| a.0.total_cmp(&b.0));
        return (two, true);
    }
    let before = cuts.iter().rev().find(|c| c.0 < tq).copied();
    let after = cuts.iter().find(|c| c.0 > tq).copied();
    (before.into_iter().chain(after).collect(), false)
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "TrimSketchCmd";
    let at = req_vec2(cmd, p, "at")?;
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let ci = curve_arg(sk, p, cmd)?;
        let id = sk.curves.get(ci).map(|c| c.id.clone()).unwrap_or_default();
        if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
            let r = super::sketch_freeform::trim_or_break(sk, ci, at, true, cmd)?;
            doc.prune_model_params();
            return Ok(json!({"trimmed": id, "pieces": r["curves"]}));
        }
        let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
        let tq = sh.param(sh.project(at));
        let (cuts, full) = bracket(sk, ci, tq);
        if cuts.is_empty() {
            // Nothing crosses it: the whole curve goes.
            sk.remove_curves(&[ci]);
            doc.prune_model_params();
            return Ok(json!({"deleted": id}));
        }
        let ts: Vec<f64> = cuts.iter().map(|c| c.0).collect();
        let (pieces, pts) = split_at(sk, ci, &ts)?;
        // The piece containing tq.
        let k = if full {
            let (a, b) = (ts[0], ts[1]);
            if tq > a && tq < b { 0 } else { 1 }
        } else {
            ts.iter().filter(|t| **t < tq).count()
        };
        for (pi, c) in pts.iter().zip(&cuts) {
            pin(sk, *pi, c.2)?;
        }
        let gone = pieces.get(k).copied().ok_or_else(|| bad(cmd, "piece"))?;
        let keep: Vec<usize> = pieces.iter().copied().filter(|x| *x != gone).collect();
        // The original curve keeps its id (and constraints): swap geometry into it if needed.
        let gone = match keep.first() {
            Some(&other) if gone == ci => {
                let ka = sk.curves.get(ci).map(|c| c.kind.clone());
                let kb = sk.curves.get(other).map(|c| c.kind.clone());
                if let (Some(ka), Some(kb)) = (ka, kb) {
                    if let Some(c) = sk.curves.get_mut(ci) {
                        c.kind = kb;
                    }
                    if let Some(c) = sk.curves.get_mut(other) {
                        c.kind = ka;
                    }
                }
                other
            }
            _ => gone,
        };
        let keep: Vec<usize> = pieces.iter().copied().filter(|x| *x != gone).collect();
        tie(sk, &keep)?;
        sk.remove_curves(&[gone]);
        doc.prune_model_params();
        Ok(json!({"trimmed": id}))
    })?;
    Ok(json!({"result": out, "sketch": info}))
}

fn break_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "BreakSketchCmd";
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let ci = curve_arg(sk, p, cmd)?;
        if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
            let at = req_vec2(cmd, p, "at")?;
            let r = super::sketch_freeform::trim_or_break(sk, ci, at, false, cmd)?;
            return Ok(r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default());
        }
        let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
        let (ts, cutters): (Vec<f64>, Vec<Option<usize>>) = if let Some(list) = p.get("points").and_then(Value::as_array) {
            let mut v: Vec<f64> = list.iter().take(1000).filter_map(vec2).map(|q| sh.param(sh.project(q))).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let n = v.len();
            (v, vec![None; n])
        } else {
            let at = req_vec2(cmd, p, "at")?;
            let (cuts, _) = bracket(sk, ci, sh.param(sh.project(at)));
            (cuts.iter().map(|c| c.0).collect(), cuts.iter().map(|c| Some(c.2)).collect())
        };
        let (t0, t1) = sh.range();
        if ts.is_empty() || (!sh.is_full() && ts.iter().any(|t| *t <= t0 || *t >= t1)) {
            return Err(bad(cmd, "nothing to break at there"));
        }
        let (pieces, pts) = split_at(sk, ci, &ts)?;
        for (pi, c) in pts.iter().zip(&cutters) {
            if let Some(c) = c {
                pin(sk, *pi, *c)?;
            }
        }
        tie(sk, &pieces)?;
        Ok(ids_of(sk, &pieces))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

fn extend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ExtendSketchCmd";
    let at = req_vec2(cmd, p, "at")?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let ci = curve_arg(sk, p, cmd)?;
        let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
        let (pa, pb) = match sk.curves.get(ci).map(|c| c.kind.clone()) {
            Some(CurveKind::Line { a, b }) | Some(CurveKind::Arc { a, b, .. }) => (a, b),
            _ => return Err(bad(cmd, "only lines and arcs can be extended")),
        };
        let (qa, qb) = (sk.point(pa).unwrap_or_default(), sk.point(pb).unwrap_or_default());
        let at_b = at.dist(qb) <= at.dist(qa);
        let end = if at_b { pb } else { pa };
        let shared = sk.curves.iter().enumerate().filter(|(i, c)| *i != ci && uses(c, end)).count();
        if shared > 0 || sk.points.get(end).is_some_and(|q| q.fixed) {
            return Err(bad(cmd, "that end is connected to other geometry"));
        }
        // Candidate hits on the unbounded curve beyond the chosen end.
        let unbounded = match sh {
            Shape::Line { .. } => sh,
            Shape::Round { c, r, start, .. } => Shape::Round { c, r, start, sweep: TAU },
        };
        let (_, t1) = sh.range();
        let mut best: Option<(f64, Vec2, usize)> = None;
        for (oi, _) in sk.curves.iter().enumerate() {
            if oi == ci {
                continue;
            }
            let Some(other) = sk.shape(oi) else { continue };
            for q in intersections(&unbounded, &other) {
                if on_bounded(&other, q).is_none() {
                    continue;
                }
                let t = sh.param(q);
                let gap = match sh {
                    Shape::Line { a, b } => {
                        let l = a.dist(b);
                        if at_b { (t - 1.0) * l } else { -t * l }
                    }
                    Shape::Round { r, .. } => {
                        if at_b {
                            (t - t1) * r
                        } else {
                            (TAU - t) * r
                        }
                    }
                };
                if gap > TOL * 10.0 && best.is_none_or(|b| gap < b.0) {
                    best = Some((gap, q, oi));
                }
            }
        }
        let (_, q, oi) = best.ok_or_else(|| bad(cmd, "nothing to extend to"))?;
        if let Some(pt) = sk.points.get_mut(end) {
            pt.pos = q;
        }
        pin(sk, end, oi)?;
        Ok(sk.points.get(end).map(|x| x.id.clone()).unwrap_or_default())
    })?;
    Ok(json!({"moved": out, "sketch": info}))
}

fn uses(c: &Curve, p: usize) -> bool {
    c.kind.uses(p)
}

// ---------------------------------------------------------------------------------------------
// Corners: fillet and chamfer

/// A corner of two lines: (line a, line b, corner point, far end of a, far end of b).
fn corner(sk: &Sketch, p: &Value, cmd: &str) -> Result<(usize, usize, usize, usize, usize)> {
    let ends = |l: usize| match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    let (la, lb) = if let Some(r) = str_(p, "point") {
        let q = sk.resolve_point(r).ok_or_else(|| bad(cmd, format!("unknown point `{r}`")))?;
        let lines: Vec<usize> = (0..sk.curves.len()).filter(|i| ends(*i).is_some_and(|(a, b)| a == q || b == q) && !sk.is_linked_curve(*i)).collect();
        match lines[..] {
            [a, b] => (a, b),
            _ => return Err(bad(cmd, "the corner must join exactly two lines")),
        }
    } else {
        let a = str_(p, "a").and_then(|i| sk.curve_index(i)).ok_or_else(|| bad(cmd, "`a` must be a line"))?;
        let b = str_(p, "b").and_then(|i| sk.curve_index(i)).ok_or_else(|| bad(cmd, "`b` must be a line"))?;
        (a, b)
    };
    let (a0, a1) = ends(la).ok_or_else(|| bad(cmd, "fillets and chamfers work on two lines"))?;
    let (b0, b1) = ends(lb).ok_or_else(|| bad(cmd, "fillets and chamfers work on two lines"))?;
    let shared = [a0, a1].into_iter().find(|x| *x == b0 || *x == b1).ok_or_else(|| bad(cmd, "the lines must share a corner point"))?;
    let fa = if a0 == shared { a1 } else { a0 };
    let fb = if b0 == shared { b1 } else { b0 };
    Ok((la, lb, shared, fa, fb))
}

/// Replace the corner end of line `l` with point `np`.
fn reattach(sk: &mut Sketch, l: usize, corner: usize, np: usize) {
    if let Some(Curve { kind: CurveKind::Line { a, b }, .. }) = sk.curves.get_mut(l) {
        if *a == corner {
            *a = np;
        } else if *b == corner {
            *b = np;
        }
    }
}

/// Points at distances `da` along a and `db` along b from the corner; lines shortened to them.
fn cut_corner(sk: &mut Sketch, la: usize, lb: usize, c: usize, fa: usize, fb: usize, da: f64, db: f64, cmd: &str) -> Result<(usize, usize, Vec2)> {
    let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
    if !(da > 1e-9 && db > 1e-9 && da < pc.dist(pa) - 1e-9 && db < pc.dist(pb) - 1e-9) {
        return Err(bad(cmd, "too large for these lines"));
    }
    let u = (pa - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let v = (pb - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let ta = sk.add_point(pc + u * da, None)?;
    let tb = sk.add_point(pc + v * db, None)?;
    reattach(sk, la, c, ta);
    reattach(sk, lb, c, tb);
    let used = sk.curves.iter().any(|x| uses(x, c));
    if !used && c != 0 {
        sk.remove_points(&[c]);
    }
    // Indices may have shifted when the corner point was removed.
    let fix = |i: usize| if !used && c != 0 && i > c { i - 1 } else { i };
    Ok((fix(ta), fix(tb), pc))
}

fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FilletSketchCmd";
    let r_expr = expr(p, "radius").ok_or_else(|| bad(cmd, "`radius` must be a number or an expression"))?;
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let r = doc.eval(&r_expr, solvecraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("radius: {e}")))?;
        if !(r > 1e-9 && r < 1e8) {
            return Err(bad(cmd, "radius must be positive"));
        }
        let (la, lb, c, fa, fb) = corner(sk, p, cmd)?;
        let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
        let u = (pa - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
        let v = (pb - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
        let half = 0.5 * u.cross(v).atan2(u.dot(v)).abs();
        if half < 1e-6 || half > std::f64::consts::FRAC_PI_2 - 1e-9 {
            return Err(bad(cmd, "the lines are parallel"));
        }
        let d = r / half.tan();
        let bis = (u + v).normalized().ok_or_else(|| bad(cmd, "the lines are parallel"))?;
        let center = pc + bis * (r / half.sin());
        let (ta, tb, _) = cut_corner(sk, la, lb, c, fa, fb, d, d, cmd)?;
        let (qa, qb) = (sk.point(ta).unwrap_or_default(), sk.point(tb).unwrap_or_default());
        let ccw = (qa - center).cross(qb - center) > 0.0;
        let arc = if ccw {
            sk.add_arc(center, qa, qb, [None, Some(ta), Some(tb)], None)?
        } else {
            sk.add_arc(center, qb, qa, [None, Some(tb), Some(ta)], None)?
        };
        let mut cons = vec![add_c(sk, ConstraintKind::Tangent { a: la, b: arc })?, add_c(sk, ConstraintKind::Tangent { a: lb, b: arc })?];
        let pname = doc.new_model_param(&r_expr, "mm");
        cons.push(sk.add_constraint(ConstraintKind::Radius { c: arc, value: r }, Some(pname.clone()))?);
        Ok((ids_of(sk, &[arc]), cons, pname))
    })?;
    Ok(json!({"curves": out.0, "constraints": out.1, "param": out.2, "sketch": info}))
}

fn chamfer(s: &mut Session, p: &Value, cmd: &str, dists: impl FnOnce(f64) -> Result<(f64, f64)>) -> Result<Value> {
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (la, lb, c, fa, fb) = corner(sk, p, cmd)?;
        let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
        let (u, v) = ((pa - pc).normalized().unwrap_or(Vec2::X), (pb - pc).normalized().unwrap_or(Vec2::Y));
        let corner_angle = u.cross(v).atan2(u.dot(v)).abs();
        let (da, db) = dists(corner_angle)?;
        let (ta, tb, _) = cut_corner(sk, la, lb, c, fa, fb, da, db, cmd)?;
        let l = sk.add_line_pts(ta, tb, None)?;
        Ok(ids_of(sk, &[l]))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

fn chamfer_equal(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ChamferSketchEqualDistance";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    chamfer(s, p, cmd, |_| Ok((d, d)))
}

fn chamfer_two(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ChamferSketchDistanceDistance";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    let d2 = num(p, "distance2").ok_or_else(|| bad(cmd, "`distance2` must be a number"))?;
    chamfer(s, p, cmd, |_| Ok((d, d2)))
}

fn chamfer_dist_angle(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ChamferSketchDistanceAngle";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    let a = num(p, "angle").ok_or_else(|| bad(cmd, "`angle` must be a number (degrees)"))?.to_radians();
    chamfer(s, p, cmd, |phi| {
        // Triangle corner–Ta–Tb: angle phi at the corner, `a` at Ta.
        let opp = std::f64::consts::PI - a - phi;
        if !(a > 1e-6 && opp > 1e-6) {
            return Err(bad(cmd, "that angle does not meet the other line"));
        }
        Ok((d, d * a.sin() / opp.sin()))
    })
}

// ---------------------------------------------------------------------------------------------
// Offset

/// Curves of the connected chain through `start` (joined where exactly two curves meet), in
/// order, with whether each is walked from its stored start (`a`) to its end (`b`).
fn chain(sk: &Sketch, start: usize, pool: &[usize]) -> (Vec<(usize, bool)>, bool) {
    let ends = |c: usize| match sk.curves.get(c).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) | Some(CurveKind::Arc { a, b, .. }) => Some((*a, *b)),
        _ => None,
    };
    let Some((s0, s1)) = ends(start) else { return (vec![(start, true)], true) };
    let next = |from: usize, at: usize, used: &[usize]| -> Option<usize> {
        let users: Vec<usize> = pool.iter().copied().filter(|c| ends(*c).is_some_and(|(a, b)| a == at || b == at)).collect();
        if users.len() != 2 {
            return None;
        }
        users.into_iter().find(|c| *c != from && !used.contains(c))
    };
    let mut fwd = vec![(start, true)];
    let mut used = vec![start];
    let (mut cur, mut at) = (start, s1);
    let mut closed = false;
    while let Some(n) = next(cur, at, &used) {
        let Some((a, b)) = ends(n) else { break };
        let forward = a == at;
        fwd.push((n, forward));
        used.push(n);
        cur = n;
        at = if forward { b } else { a };
        if used.len() > 10_000 {
            break;
        }
    }
    if at == s0 && fwd.len() > 1 {
        closed = true;
    } else {
        // Walk backwards from the start.
        let (mut cur, mut at) = (start, s0);
        let mut back = Vec::new();
        while let Some(n) = next(cur, at, &used) {
            let Some((a, b)) = ends(n) else { break };
            let forward = b == at;
            back.push((n, forward));
            used.push(n);
            cur = n;
            at = if forward { a } else { b };
            if used.len() > 10_000 {
                break;
            }
        }
        back.reverse();
        back.extend(fwd);
        fwd = back;
    }
    (fwd, closed)
}

/// An offset piece: shape walked in chain direction, offset by `d` to the left.
#[derive(Clone, Copy)]
enum Piece {
    Line(Vec2, Vec2),
    /// Centre point index, centre, radius, start point, end point (walk direction), ccw walk.
    Arc(usize, Vec2, f64, Vec2, Vec2, bool),
}

fn offset_piece(sk: &Sketch, c: usize, forward: bool, d: f64) -> Option<Piece> {
    match sk.curves.get(c)?.kind {
        CurveKind::Line { a, b } => {
            let (mut p, mut q) = (sk.point(a)?, sk.point(b)?);
            if !forward {
                std::mem::swap(&mut p, &mut q);
            }
            let n = (q - p).normalized()?.perp();
            Some(Piece::Line(p + n * d, q + n * d))
        }
        CurveKind::Arc { c: ci, a, b } => {
            let (cc, pa, pb) = (sk.point(ci)?, sk.point(a)?, sk.point(b)?);
            let r = cc.dist(pa);
            // Walking counter-clockwise, left is toward the centre.
            let nr = if forward { r - d } else { r + d };
            if nr <= 1e-9 {
                return None;
            }
            let at = |q: Vec2| cc + (q - cc).normalized().unwrap_or(Vec2::X) * nr;
            let (s, e) = if forward { (at(pa), at(pb)) } else { (at(pb), at(pa)) };
            Some(Piece::Arc(ci, cc, nr, s, e, forward))
        }
        _ => None,
    }
}

fn piece_shape(p: &Piece) -> Shape {
    match *p {
        Piece::Line(a, b) => Shape::Line { a, b },
        Piece::Arc(_, c, r, ..) => Shape::Round { c, r, start: 0.0, sweep: TAU },
    }
}

fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "Offset";
    let ids = string_list(p, "curves");
    if ids.is_empty() {
        return Err(bad(cmd, "`curves` must list curves"));
    }
    let d0 = num(p, "distance").filter(|d| d.abs() > 1e-9).ok_or_else(|| bad(cmd, "`distance` must be a non-zero number"))?;
    let side = p.get("side").and_then(vec2);
    let use_chain = bool_(p, "chain").unwrap_or(true);
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let sel: Vec<usize> = ids.iter().map(|i| sk.curve_index(i).ok_or_else(|| bad(cmd, format!("unknown curve `{i}`")))).collect::<Result<_>>()?;
        let first = *sel.first().ok_or_else(|| bad(cmd, "no curves"))?;
        let mut made = Vec::new();
        let mut cons = Vec::new();
        // Circles offset on their own.
        if let Some(CurveKind::Circle { c, r }) = sk.curves.get(first).map(|c| c.kind.clone()) {
            let cc = sk.point(c).unwrap_or_default();
            let grow = match side {
                Some(q) => q.dist(cc) > r,
                None => d0 < 0.0,
            };
            let nr = if grow { r + d0.abs() } else { r - d0.abs() };
            if nr <= 1e-9 {
                return Err(bad(cmd, "the offset circle would have no size"));
            }
            let ci = sk.add_circle(cc, nr, Some(c), None)?;
            made.push(ci);
            return Ok((ids_of(sk, &made), cons));
        }
        let pool: Vec<usize> = if use_chain {
            (0..sk.curves.len())
                .filter(|i| sk.curves.get(*i).is_some_and(|c| c.construction == sk.curves.get(first).is_some_and(|f| f.construction)))
                .collect()
        } else {
            sel.clone()
        };
        let (order, closed) = chain(sk, first, &pool);
        // Side: toward `side` (compare both offsets' distance to it), else the sign.
        let d = match side {
            Some(q) => {
                let dist = |dd: f64| {
                    offset_piece(sk, first, order.iter().find(|x| x.0 == first).is_none_or(|x| x.1), dd)
                        .map(|pc| piece_shape(&pc).dist(q))
                        .unwrap_or(f64::MAX)
                };
                if dist(d0.abs()) <= dist(-d0.abs()) { d0.abs() } else { -d0.abs() }
            }
            None => d0,
        };
        let pieces: Vec<Piece> =
            order.iter().map(|(c, f)| offset_piece(sk, *c, *f, d)).collect::<Option<_>>().ok_or_else(|| bad(cmd, "the offset collapses an arc"))?;
        let n = pieces.len();
        let start_of = |p: &Piece| match *p {
            Piece::Line(a, _) => a,
            Piece::Arc(_, _, _, s, _, _) => s,
        };
        let end_of = |p: &Piece| match *p {
            Piece::Line(_, b) => b,
            Piece::Arc(_, _, _, _, e, _) => e,
        };
        // Junction points between consecutive pieces.
        let joint = |x: &Piece, y: &Piece| -> Vec2 {
            let (e, s0) = (end_of(x), start_of(y));
            if e.dist(s0) < TOL * 10.0 {
                return e;
            }
            let guess = (e + s0) * 0.5;
            intersections(&piece_shape(x), &piece_shape(y)).into_iter().min_by(|a, b| a.dist(guess).total_cmp(&b.dist(guess))).unwrap_or(guess)
        };
        let mut joints: Vec<Vec2> = Vec::new();
        joints.push(if closed { joint(&pieces[n - 1], &pieces[0]) } else { start_of(&pieces[0]) });
        for k in 0..n.saturating_sub(1) {
            joints.push(joint(&pieces[k], &pieces[k + 1]));
        }
        if !closed {
            joints.push(end_of(&pieces[n - 1]));
        }
        let pts: Vec<usize> = joints.iter().map(|q| sk.add_point(*q, None)).collect::<std::result::Result<_, _>>()?;
        for (k, pc) in pieces.iter().enumerate() {
            let (pa, pb) = (pts[k], if closed { pts[(k + 1) % n] } else { pts[k + 1] });
            let (orig, _) = order[k];
            match *pc {
                Piece::Line(..) => {
                    let l = sk.add_line_pts(pa, pb, None)?;
                    cons.push(add_c(sk, ConstraintKind::Parallel { a: orig, b: l })?);
                    made.push(l);
                }
                Piece::Arc(ci, cc, _, _, _, ccw) => {
                    let (qa, qb) = (sk.point(pa).unwrap_or_default(), sk.point(pb).unwrap_or_default());
                    let a = if ccw {
                        sk.add_arc(cc, qa, qb, [Some(ci), Some(pa), Some(pb)], None)?
                    } else {
                        sk.add_arc(cc, qb, qa, [Some(ci), Some(pb), Some(pa)], None)?
                    };
                    made.push(a);
                }
            }
        }
        Ok((ids_of(sk, &made), cons))
    })?;
    Ok(json!({"curves": out.0, "constraints": out.1, "sketch": info}))
}

// ---------------------------------------------------------------------------------------------
// Copies: mirror, patterns, move, scale

/// Curves and lone points named in `entities`.
fn entities(sk: &Sketch, p: &Value, cmd: &str) -> Result<(Vec<usize>, Vec<usize>)> {
    let ids = string_list(p, "entities");
    if ids.is_empty() {
        return Err(bad(cmd, "`entities` must list sketch curves or points"));
    }
    let (mut cs, mut ps) = (Vec::new(), Vec::new());
    for id in &ids {
        if let Some(c) = sk.curve_index(id) {
            cs.push(c);
        } else if let Some(q) = sk.resolve_point(id) {
            ps.push(q);
        } else {
            return Err(bad(cmd, format!("unknown sketch entity `{id}`")));
        }
    }
    Ok((cs, ps))
}

/// Points used by curves plus lone points, in a stable order.
fn involved_points(sk: &Sketch, cs: &[usize], ps: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for c in cs {
        if let Some(k) = sk.curves.get(*c).map(|c| &c.kind) {
            v.extend(k.point_ids());
        }
    }
    v.extend(ps);
    let mut out = Vec::new();
    for q in v {
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

/// Copy curves and points through `f`. `flip` reverses arcs (mirroring). `keep(p)`: reuse the
/// original point instead of copying it (points on a mirror line). Returns the new curves and
/// the point map (original → copy).
fn copy_through(
    sk: &mut Sketch,
    cs: &[usize],
    ps: &[usize],
    f: &dyn Fn(Vec2) -> Vec2,
    flip: bool,
    keep: &dyn Fn(Vec2) -> bool,
    scale_r: f64,
) -> Result<(Vec<usize>, Vec<(usize, usize)>)> {
    let pts = involved_points(sk, cs, ps);
    let mut map: Vec<(usize, usize)> = Vec::new();
    for q in &pts {
        let pos = sk.point(*q).unwrap_or_default();
        let n = if keep(pos) { *q } else { sk.add_point(f(pos), None)? };
        map.push((*q, n));
    }
    let m = |q: usize| map.iter().find(|x| x.0 == q).map(|x| x.1).unwrap_or(q);
    let mut made = Vec::new();
    for c in cs {
        let Some(cu) = sk.curves.get(*c).cloned() else { continue };
        let mut kind = cu.kind.clone();
        kind.map_points(&m);
        let (kind, prefix) = match cu.kind {
            CurveKind::Ellipse { r, .. } => {
                if let CurveKind::Ellipse { r: slot, .. } = &mut kind {
                    *slot = r * scale_r;
                }
                (kind, "e")
            }
            CurveKind::Spline { .. } => (kind, "s"),
            CurveKind::Conic { .. } => (kind, "k"),
            CurveKind::Line { a, b } => (CurveKind::Line { a: m(a), b: m(b) }, "l"),
            CurveKind::Circle { c, r } => (CurveKind::Circle { c: m(c), r: r * scale_r }, "c"),
            CurveKind::Arc { c, a, b } => {
                if flip {
                    (CurveKind::Arc { c: m(c), a: m(b), b: m(a) }, "a")
                } else {
                    (CurveKind::Arc { c: m(c), a: m(a), b: m(b) }, "a")
                }
            }
        };
        let id = sk.fresh(prefix);
        sk.curves.push(Curve {
            id,
            kind,
            construction: cu.construction,
            reversed: if flip { !cu.reversed } else { cu.reversed },
            link: None,
            centerline: false,
        });
        made.push(sk.curves.len() - 1);
    }
    Ok((made, map))
}

fn mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "MirrorSketchCommand";
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let lid = str_(p, "line").ok_or_else(|| bad(cmd, "`line` must be the mirror line"))?;
        let l = sk.curve_index(lid).ok_or_else(|| bad(cmd, format!("unknown curve `{lid}`")))?;
        let Some(Shape::Line { a, b }) = sk.shape(l) else { return Err(bad(cmd, "the mirror must be a line")) };
        let u = (b - a).normalized().ok_or_else(|| bad(cmd, "degenerate mirror line"))?;
        let refl = move |q: Vec2| {
            let foot = a + u * (q - a).dot(u);
            foot * 2.0 - q
        };
        let on_line = move |q: Vec2| u.cross(q - a).abs() < TOL * 10.0;
        let cs: Vec<usize> = cs.into_iter().filter(|c| *c != l).collect();
        let (made, map) = copy_through(sk, &cs, &ps, &refl, true, &on_line, 1.0)?;
        let mut cons = Vec::new();
        for (o, n) in &map {
            if o != n {
                cons.push(add_c(sk, ConstraintKind::Symmetric { p: *o, q: *n, l })?);
            }
        }
        for (o, n) in cs.iter().zip(&made) {
            if matches!(sk.curves.get(*o).map(|c| &c.kind), Some(CurveKind::Circle { .. })) {
                cons.push(add_c(sk, ConstraintKind::Equal { a: *o, b: *n })?);
            }
        }
        Ok((ids_of(sk, &made), cons))
    })?;
    Ok(json!({"curves": out.0, "constraints": out.1, "sketch": info}))
}

fn count(p: &Value, k: &str, cmd: &str) -> Result<usize> {
    let n = num(p, k).ok_or_else(|| bad(cmd, format!("`{k}` must be a number")))?;
    if !(1.0..=MAX_COPIES as f64).contains(&n) {
        return Err(bad(cmd, format!("`{k}` must be 1…{MAX_COPIES}")));
    }
    Ok(n as usize)
}

fn point_or_ref(sk: &Sketch, p: &Value, k: &str, cmd: &str) -> Result<Vec2> {
    match p.get(k) {
        Some(Value::String(r)) => sk.resolve_point(r).and_then(|i| sk.point(i)).ok_or_else(|| bad(cmd, format!("unknown point `{r}`"))),
        Some(v) => vec2(v).ok_or_else(|| bad(cmd, format!("`{k}` must be [x, y] or a point id"))),
        None => Err(bad(cmd, format!("missing `{k}`"))),
    }
}

fn rotate(c: Vec2, ang: f64) -> impl Fn(Vec2) -> Vec2 {
    move |q: Vec2| {
        let d = q - c;
        let (s, co) = ang.sin_cos();
        c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }
}

fn pattern_circular(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircularSketchPatternCommand";
    let n = count(p, "count", cmd)?;
    let total = num(p, "angle").unwrap_or(360.0);
    if !(total.abs() > 1e-9 && total.abs() <= 360.0) {
        return Err(bad(cmd, "`angle` must be in (0, 360]"));
    }
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let c = point_or_ref(sk, p, "center", cmd)?;
        let full = (total.abs() - 360.0).abs() < 1e-9;
        let step = if full { total / n as f64 } else { total / (n.max(2) - 1) as f64 };
        let mut made = Vec::new();
        for k in 1..n {
            let f = rotate(c, (step * k as f64).to_radians());
            let (m, _) = copy_through(sk, &cs, &ps, &f, false, &|_| false, 1.0)?;
            made.extend(m);
        }
        Ok(ids_of(sk, &made))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

fn pattern_rect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "RectangularSketchPatternCommand";
    let n1 = count(p, "count", cmd)?;
    let s1 = num(p, "spacing").ok_or_else(|| bad(cmd, "`spacing` must be a number"))?;
    let n2 = if p.get("count2").is_some() { count(p, "count2", cmd)? } else { 1 };
    let s2 = num(p, "spacing2").unwrap_or(s1);
    if n1 * n2 > MAX_COPIES {
        return Err(bad(cmd, "too many copies"));
    }
    let d1 = p.get("dir").and_then(vec2).and_then(Vec2::normalized).unwrap_or(Vec2::X);
    let d2 = d1.perp();
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let mut made = Vec::new();
        for i in 0..n1 {
            for j in 0..n2 {
                if i == 0 && j == 0 {
                    continue;
                }
                let off = d1 * (s1 * i as f64) + d2 * (s2 * j as f64);
                let (m, _) = copy_through(sk, &cs, &ps, &move |q| q + off, false, &|_| false, 1.0)?;
                made.extend(m);
            }
        }
        Ok(ids_of(sk, &made))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

/// Move the points of the entities (in place) through `f`; circle radii scale by `k`.
fn transform_in_place(sk: &mut Sketch, cs: &[usize], ps: &[usize], f: &dyn Fn(Vec2) -> Vec2, k: f64, cmd: &str) -> Result<()> {
    let pts = involved_points(sk, cs, ps);
    if pts.iter().any(|q| sk.points.get(*q).is_some_and(|x| x.fixed || x.link.is_some())) {
        return Err(bad(cmd, "fixed or projected geometry cannot move"));
    }
    for q in pts {
        if let Some(x) = sk.points.get_mut(q) {
            x.pos = f(x.pos);
        }
    }
    for c in cs {
        if let Some(Curve { kind: CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }, .. }) = sk.curves.get_mut(*c) {
            *r *= k;
        }
    }
    Ok(())
}

fn move_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move";
    let t = p.get("translate").and_then(vec2).unwrap_or(Vec2::ZERO);
    let ang = num(p, "angle").unwrap_or(0.0).to_radians();
    let copy = bool_(p, "copy").unwrap_or(false);
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let c = if p.get("center").is_some() { point_or_ref(sk, p, "center", cmd)? } else { Vec2::ZERO };
        let rot = rotate(c, ang);
        let f = move |q: Vec2| rot(q) + t;
        if copy {
            let (m, _) = copy_through(sk, &cs, &ps, &f, false, &|_| false, 1.0)?;
            return Ok(ids_of(sk, &m));
        }
        transform_in_place(sk, &cs, &ps, &f, 1.0, cmd)?;
        Ok(ids_of(sk, &cs))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchScaleCmd";
    let k = num(p, "factor").filter(|k| *k > 1e-9 && *k < 1e6).ok_or_else(|| bad(cmd, "`factor` must be a positive number"))?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let b = if p.get("base").is_some() { point_or_ref(sk, p, "base", cmd)? } else { Vec2::ZERO };
        let f = move |q: Vec2| b + (q - b) * k;
        transform_in_place(sk, &cs, &ps, &f, k, cmd)?;
        Ok(ids_of(sk, &cs))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

#[cfg(test)]
#[path = "sketch_modify_tests.rs"]
mod tests;
