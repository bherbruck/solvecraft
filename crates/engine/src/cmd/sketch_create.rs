//! More sketch creation tools: midpoint line, tangent arc, tangent circles, center-point and
//! arc slots.

use serde_json::{Value, json};
use solvecraft_geom::Vec2;
use solvecraft_sketch::{ConstraintKind, CurveKind, LinkKind, LinkSource, Sketch};

use super::sketch::{add_c, circumcircle, edit, ids_of, mark_construction, req_parg, result, slot};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, num, req_vec2, str_, string_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] =
    &[
        CommandSpec::new("SketchMidpointLine", "Midpoint Line", midpoint_line)
            .at("SKETCH", "CREATE")
            .icon("line_mid")
            .enabled(in_sketch)
            .params("mid: [x,y] or point ref, end: [x,y]; construction?"),
        CommandSpec::new("ArcTangent", "Tangent Arc", arc_tangent).at("SKETCH", "CREATE").icon("arc_tangent").enabled(in_sketch).params(
            "start: end point of a line or arc (\"l1.end\"), end: [x,y]; curve?: the curve to be tangent to (default: the one ending at start)",
        ),
        CommandSpec::new("CircleTanTanRadius", "2-Tangent Circle", circle_tan_tan)
            .at("SKETCH", "CREATE")
            .icon("circle_tt")
            .enabled(in_sketch)
            .params("curves: [two lines/circles/arcs], radius | diameter: expr, near: [x,y] (which of the solutions)"),
        CommandSpec::new("CircleThreeTangent", "3-Tangent Circle", circle_three_tan)
            .at("SKETCH", "CREATE")
            .icon("circle_ttt")
            .enabled(in_sketch)
            .params("curves: [three lines/circles/arcs], near: [x,y] (which of the solutions)"),
        CommandSpec::new("ShapeSlotCenterPoint", "Center Point Slot", slot_center_point)
            .at("SKETCH", "CREATE")
            .icon("slot")
            .enabled(in_sketch)
            .params("center: slot centre, end: one arc centre, width"),
        CommandSpec::new("ShapeArcSlotThreePoint", "Three Point Arc Slot", arc_slot_three)
            .at("SKETCH", "CREATE")
            .icon("arc_slot")
            .enabled(in_sketch)
            .params("start, through, end: points on the centre arc, width"),
        CommandSpec::new("ShapeArcSlotCenterTwoPoint", "Center Point Arc Slot", arc_slot_center)
            .at("SKETCH", "CREATE")
            .icon("arc_slot")
            .enabled(in_sketch)
            .params("center, start: centre arc start, end: direction of the centre arc end (counter-clockwise), width"),
        CommandSpec::new("CircleElipse", "Ellipse", ellipse)
            .at("SKETCH", "CREATE")
            .icon("ellipse")
            .enabled(in_sketch)
            .params("center, major: end of the major axis [x,y], minor: a point the ellipse passes through on the minor side [x,y] | minor_radius"),
        CommandSpec::new("DrawSpline", "Fit Point Spline", spline_fit)
            .at("SKETCH", "CREATE")
            .icon("spline")
            .enabled(in_sketch)
            .params("points: [[x,y] or point refs…] (2…500): the spline passes through them"),
        CommandSpec::new("DrawCVMSpline3D", "Control Point Spline", spline_cv3)
            .at("SKETCH", "CREATE")
            .icon("spline_cv")
            .enabled(in_sketch)
            .params("points: control points (2…500), cubic"),
        CommandSpec::new("DrawCVMSpline5D", "Control Point Spline (Degree 5)", spline_cv5)
            .at("SKETCH", "CREATE")
            .icon("spline_cv")
            .enabled(in_sketch)
            .params("points: control points (2…500), degree 5"),
        CommandSpec::new("ConicCurveCmd", "Conic Curve", conic)
            .at("SKETCH", "CREATE")
            .icon("conic")
            .enabled(in_sketch)
            .params("start, end, apex: [x,y] or point refs; rho?: 0…1 (default 0.5: parabola)"),
        CommandSpec::new("BlendG1CurveSketchCmd", "Blend Curve", blend)
            .at("SKETCH", "MODIFY")
            .icon("blend")
            .enabled(in_sketch)
            .params("a, b: end points of two curves (\"l1.end\", \"a2.start\"): a smooth (tangent) spline joining them"),
        CommandSpec::new("sketch.centerline", "Centerline", centerline)
            .enabled(in_sketch)
            .params("curves: [line ids], value?: bool (default toggles): centerline line type (not in profiles; an axis)"),
        CommandSpec::new("MTextCmd", "Text", text)
            .at("SKETCH", "CREATE")
            .icon("text")
            .enabled(in_sketch)
            .params("text: string (\\n breaks lines), at: baseline start [x,y], height?: capital height mm (default 5), angle?: deg"),
        CommandSpec::new("sketch.edit_text", "Edit Text", edit_text)
            .enabled(in_sketch)
            .params("link: the text's link id (or entity: one of its curves), text?, at?, height?, angle?: deg"),
    ];

fn text_source(p: &Value, cmd: &str, old: Option<(String, Vec2, f64, f64)>) -> Result<LinkSource> {
    let (t0, at0, h0, a0) = old.unwrap_or((String::new(), Vec2::ZERO, 5.0, 0.0));
    let text = str_(p, "text").map(str::to_string).unwrap_or(t0);
    if text.trim().is_empty() {
        return Err(bad(cmd, "`text` must not be empty"));
    }
    if text.chars().count() > solvecraft_sketch::MAX_TEXT_CHARS {
        return Err(bad(cmd, "text too long"));
    }
    let at = p.get("at").and_then(crate::params::vec2).unwrap_or(at0);
    let height = num(p, "height").unwrap_or(h0);
    if !(height > 1e-6 && height < 1e5) {
        return Err(bad(cmd, "`height` must be positive"));
    }
    let angle = num(p, "angle").map(f64::to_radians).unwrap_or(a0);
    Ok(LinkSource::Text { text, at, height, angle })
}

fn text(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "MTextCmd";
    if p.get("at").is_none() {
        return Err(bad(cmd, "`at` must be the baseline start [x, y]"));
    }
    let src = text_source(p, cmd, None)?;
    let id = s.active_sketch.ok_or_else(|| bad(cmd, "no sketch is being edited"))?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let l = super::sketch_project::add_link(s, &doc, id, &mut sk, LinkKind::Text, src)?;
    let curves = ids_of(&sk, &sk.link_curves(&l));
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(json!({"link": l, "curves": curves}))
}

fn edit_text(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.edit_text";
    let id = s.active_sketch.ok_or_else(|| bad(cmd, "no sketch is being edited"))?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let l = match (str_(p, "link"), str_(p, "entity")) {
        (Some(l), _) => l.to_string(),
        (None, Some(e)) => sk.curves.iter().find(|c| c.id == e).and_then(|c| c.link.clone()).ok_or_else(|| bad(cmd, format!("`{e}` is not text")))?,
        _ => return Err(bad(cmd, "give `link` or `entity`")),
    };
    let old = match sk.link(&l).map(|x| (x.kind, x.source.clone())) {
        Some((LinkKind::Text, LinkSource::Text { text, at, height, angle })) => (text, at, height, angle),
        _ => return Err(bad(cmd, format!("`{l}` is not text"))),
    };
    let src = text_source(p, cmd, Some(old))?;
    let LinkSource::Text { text, at, height, angle } = &src else { return Err(bad(cmd, "text")) };
    let geom = solvecraft_sketch::text_geometry(text, *at, *height, *angle)?;
    sk.set_link_source(&l, src.clone());
    sk.update_link(&l, &geom)?;
    let curves = ids_of(&sk, &sk.link_curves(&l));
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(json!({"link": l, "curves": curves}))
}

/// The curve ending at point `pi` and the direction leaving it there.
fn end_of(sk: &Sketch, pi: usize) -> Option<(usize, Vec2)> {
    sk.curves.iter().enumerate().filter(|(_, c)| !c.construction).find_map(|(i, _)| leaving_dir(sk, i, pi).map(|d| (i, d)))
}

fn blend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "BlendG1CurveSketchCmd";
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let pa = req_parg(sk, p, "a", cmd)?.idx().ok_or_else(|| bad(cmd, "`a` must be the end point of a curve"))?;
        let pb = req_parg(sk, p, "b", cmd)?.idx().ok_or_else(|| bad(cmd, "`b` must be the end point of a curve"))?;
        let (ca, ta) = end_of(sk, pa).ok_or_else(|| bad(cmd, "no curve ends at `a`"))?;
        let (cb, tb) = end_of(sk, pb).ok_or_else(|| bad(cmd, "no curve ends at `b`"))?;
        let (qa, qb) = (sk.point(pa).unwrap_or_default(), sk.point(pb).unwrap_or_default());
        let l = qa.dist(qb);
        if l < 1e-9 || ca == cb && pa == pb {
            return Err(bad(cmd, "the ends must differ"));
        }
        let first_new = sk.points.len();
        let c1 = sk.add_point(qa + ta * (l / 3.0), None)?;
        let c2 = sk.add_point(qb + tb * (l / 3.0), None)?;
        let sp = sk.add_curve(CurveKind::Spline { pts: vec![pa, c1, c2, pb], control: true, degree: 3 }, None)?;
        let cons = vec![add_c(sk, ConstraintKind::Tangent { a: ca, b: sp })?, add_c(sk, ConstraintKind::Tangent { a: cb, b: sp })?];
        solve_new_only(sk, first_new);
        Ok((ids_of(sk, &[sp]), cons))
    })?;
    Ok(result(out, info))
}

fn centerline(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.centerline";
    let ids = string_list(p, "curves");
    if ids.is_empty() {
        return Err(bad(cmd, "`curves` must list lines"));
    }
    let want = crate::params::bool_(p, "value");
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        for id in &ids {
            let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            let cu = sk.curves.get_mut(c).ok_or_else(|| bad(cmd, "curve"))?;
            if !matches!(cu.kind, CurveKind::Line { .. }) {
                return Err(bad(cmd, format!("`{id}` is not a line")));
            }
            cu.centerline = want.unwrap_or(!cu.centerline);
            if cu.centerline {
                cu.construction = false;
            }
        }
        Ok(ids.len())
    })?;
    Ok(json!({"changed": n, "sketch": info}))
}

fn ellipse(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircleElipse";
    let (c, m) = (req_vec2(cmd, p, "center")?, req_vec2(cmd, p, "major")?);
    let u = (m - c).normalized().ok_or_else(|| bad(cmd, "the major axis end must differ from the centre"))?;
    let a = c.dist(m);
    let r = match (num(p, "minor_radius"), p.get("minor").and_then(crate::params::vec2)) {
        (Some(r), _) => r,
        (None, Some(q)) => {
            // Minor radius so the ellipse passes through q.
            let d = q - c;
            let (x, y) = (d.dot(u), d.cross(u).abs());
            let k = 1.0 - (x / a).powi(2);
            if k <= 1e-12 {
                return Err(bad(cmd, "the minor point must lie within the major axis span"));
            }
            y / k.sqrt()
        }
        _ => return Err(bad(cmd, "needs `minor` [x,y] or `minor_radius`")),
    };
    if !(r > 1e-9 && r < 1e8) {
        return Err(bad(cmd, "the minor radius must be positive"));
    }
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let ci = sk.add_point(c, None)?;
        let mi = sk.add_point(m, None)?;
        let e = sk.add_curve(CurveKind::Ellipse { c: ci, m: mi, r }, None)?;
        mark_construction(sk, &[e], p);
        Ok((ids_of(sk, &[e]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn point_list(sk: &mut Sketch, p: &Value, cmd: &str) -> Result<Vec<usize>> {
    let list = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must be a list"))?;
    if list.len() < 2 || list.len() > solvecraft_sketch::MAX_SPLINE_POINTS {
        return Err(bad(cmd, "a spline needs 2 to 500 points"));
    }
    let mut out = Vec::new();
    for v in list {
        let a = super::sketch::parg(sk, v, cmd)?;
        let i = match a.idx() {
            Some(i) => i,
            None => sk.add_point(a.pos(), None)?,
        };
        if out.last() == Some(&i) || out.last().and_then(|l| sk.point(*l)).is_some_and(|q| q.dist(a.pos()) < 1e-9) {
            return Err(bad(cmd, "consecutive points must differ"));
        }
        out.push(i);
    }
    Ok(out)
}

fn spline(s: &mut Session, p: &Value, cmd: &str, control: bool, degree: u8) -> Result<Value> {
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let pts = point_list(sk, p, cmd)?;
        let c = sk.add_curve(CurveKind::Spline { pts, control, degree }, None)?;
        mark_construction(sk, &[c], p);
        Ok((ids_of(sk, &[c]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn spline_fit(s: &mut Session, p: &Value) -> Result<Value> {
    spline(s, p, "DrawSpline", false, 3)
}
fn spline_cv3(s: &mut Session, p: &Value) -> Result<Value> {
    spline(s, p, "DrawCVMSpline3D", true, 3)
}
fn spline_cv5(s: &mut Session, p: &Value) -> Result<Value> {
    spline(s, p, "DrawCVMSpline5D", true, 5)
}

fn conic(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConicCurveCmd";
    let rho = num(p, "rho").unwrap_or(0.5);
    if !(rho > 1e-6 && rho < 1.0 - 1e-6) {
        return Err(bad(cmd, "`rho` must be between 0 and 1"));
    }
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let mut idx = Vec::new();
        for k in ["start", "end", "apex"] {
            let a = req_parg(sk, p, k, cmd)?;
            idx.push(match a.idx() {
                Some(i) => i,
                None => sk.add_point(a.pos(), None)?,
            });
        }
        let (a, b, x) = (idx[0], idx[1], idx[2]);
        if a == b || a == x || b == x {
            return Err(bad(cmd, "start, end and apex must differ"));
        }
        let c = sk.add_curve(CurveKind::Conic { a, b, apex: x, rho }, None)?;
        mark_construction(sk, &[c], p);
        Ok((ids_of(sk, &[c]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn midpoint_line(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchMidpointLine";
    let end = req_vec2(cmd, p, "end")?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let m = req_parg(sk, p, "mid", cmd)?;
        let start = m.pos() * 2.0 - end;
        if start.dist(end) < 1e-9 {
            return Err(bad(cmd, "the end must differ from the midpoint"));
        }
        let l = sk.add_line(start, end, None, None, None)?;
        let mp = match m.idx() {
            Some(i) => i,
            None => sk.add_point(m.pos(), None)?,
        };
        let k = add_c(sk, ConstraintKind::Midpoint { p: mp, l })?;
        mark_construction(sk, &[l], p);
        Ok((ids_of(sk, &[l]), vec![k]))
    })?;
    Ok(result(out, info))
}

/// Direction leaving curve `ci` at its end point `pi` (continuing the curve past that end).
fn leaving_dir(sk: &Sketch, ci: usize, pi: usize) -> Option<Vec2> {
    match sk.curves.get(ci)?.kind {
        CurveKind::Line { a, b } => {
            let (pa, pb) = (sk.point(a)?, sk.point(b)?);
            if pi == b {
                (pb - pa).normalized()
            } else if pi == a {
                (pa - pb).normalized()
            } else {
                None
            }
        }
        CurveKind::Arc { c, a, b } => {
            let (pc, q) = (sk.point(c)?, sk.point(pi)?);
            let ccw = (q - pc).perp().normalized()?;
            if pi == b {
                Some(ccw)
            } else if pi == a {
                Some(-ccw)
            } else {
                None
            }
        }
        CurveKind::Spline { ref pts, control, .. } => {
            let q: Vec<Vec2> = pts.iter().filter_map(|i| sk.point(*i)).collect();
            if pts.last() == Some(&pi) {
                solvecraft_sketch::spline_end_tangent(&q, control, true)
            } else if pts.first() == Some(&pi) {
                solvecraft_sketch::spline_end_tangent(&q, control, false).map(|t| -t)
            } else {
                None
            }
        }
        CurveKind::Conic { a, b, apex, .. } => {
            let x = sk.point(apex)?;
            if pi == b {
                (sk.point(b)? - x).normalized()
            } else if pi == a {
                (sk.point(a)? - x).normalized()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn arc_tangent(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ArcTangent";
    let end = req_vec2(cmd, p, "end")?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let st = req_parg(sk, p, "start", cmd)?;
        let pi = st.idx().ok_or_else(|| bad(cmd, "`start` must be the end point of a line or arc"))?;
        let ci = match str_(p, "curve") {
            Some(id) => sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?,
            None => sk
                .curves
                .iter()
                .enumerate()
                .find(|(i, c)| !c.construction && leaving_dir(sk, *i, pi).is_some())
                .map(|(i, _)| i)
                .ok_or_else(|| bad(cmd, "no line or arc ends at `start`"))?,
        };
        let t = leaving_dir(sk, ci, pi).ok_or_else(|| bad(cmd, "`start` is not an end of that curve"))?;
        let s0 = st.pos();
        let n = t.perp();
        let d = end - s0;
        let den = 2.0 * n.dot(d);
        if den.abs() < 1e-9 * d.len().max(1e-9) || d.len() < 1e-9 {
            return Err(bad(cmd, "the end is straight ahead: draw a line instead"));
        }
        let center = s0 + n * (d.len2() / den);
        let a = if den > 0.0 {
            sk.add_arc(center, s0, end, [None, Some(pi), None], None)?
        } else {
            let a = sk.add_arc(center, end, s0, [None, None, Some(pi)], None)?;
            if let Some(c) = sk.curves.get_mut(a) {
                c.reversed = true;
            }
            a
        };
        let k = add_c(sk, ConstraintKind::Tangent { a: ci, b: a })?;
        Ok((ids_of(sk, &[a]), vec![k]))
    })?;
    Ok(result(out, info))
}

/// Initial radius for a circle near `at` touching the curves: the mean distance to them.
fn guess_radius(sk: &Sketch, curves: &[usize], at: Vec2) -> f64 {
    let ds: Vec<f64> = curves.iter().filter_map(|c| sk.shape(*c)).map(|sh| sh.project(at).dist(at)).collect();
    let r = ds.iter().sum::<f64>() / ds.len().max(1) as f64;
    if r.is_finite() && r > 1e-6 { r } else { 1.0 }
}

fn tangent_curves(sk: &Sketch, p: &Value, cmd: &str, n: usize) -> Result<Vec<usize>> {
    let ids = string_list(p, "curves");
    if ids.len() != n {
        return Err(bad(cmd, format!("`curves` must list {n} curves")));
    }
    ids.iter().map(|id| sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))).collect()
}

/// A circle tangent to `curves`, solved from a start near `near`.
fn tangent_circle(s: &mut Session, p: &Value, cmd: &str, n: usize, radius: Option<String>) -> Result<Value> {
    let near = req_vec2(cmd, p, "near")?;
    let (out, info) = edit(s, p, cmd, true, |sk, doc| {
        let cs = tangent_curves(sk, p, cmd, n)?;
        let r0 = match &radius {
            Some(e) => doc.eval(e, solvecraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("radius: {e}")))?,
            None => guess_radius(sk, &cs, near),
        };
        if !(r0 > 1e-9 && r0 < 1e8) {
            return Err(bad(cmd, "radius must be positive"));
        }
        // Start where the circle already nearly touches: move the centre so the guessed circle
        // sits between the curves.
        let first_new = sk.points.len();
        let c = sk.add_circle(near, r0, None, None)?;
        let mut cons = Vec::new();
        for k in &cs {
            cons.push(add_c(sk, ConstraintKind::Tangent { a: *k, b: c })?);
        }
        if let Some(e) = &radius {
            let pname = doc.new_model_param(e, "mm");
            let k = sk.add_constraint(ConstraintKind::Radius { c, value: r0 }, Some(pname))?;
            cons.push(k);
        }
        solve_new_only(sk, first_new);
        Ok((ids_of(sk, &[c]), cons))
    })?;
    if info["solved"] != json!(true) {
        return Err(bad(cmd, "no tangent circle there"));
    }
    Ok(result(out, info))
}

fn circle_tan_tan(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircleTanTanRadius";
    let r = match (crate::params::expr(p, "radius"), num(p, "diameter")) {
        (Some(r), _) => r,
        (None, Some(d)) => format!("{}", d / 2.0),
        _ => return Err(bad(cmd, "needs `radius` or `diameter`")),
    };
    tangent_circle(s, p, cmd, 2, Some(r))
}

fn circle_three_tan(s: &mut Session, p: &Value) -> Result<Value> {
    tangent_circle(s, p, "CircleThreeTangent", 3, None)
}

fn width(p: &Value, cmd: &str) -> Result<f64> {
    num(p, "width").filter(|w| *w > 1e-9).ok_or_else(|| bad(cmd, "`width` must be positive"))
}

fn slot_center_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeSlotCenterPoint";
    let (c, e) = (req_vec2(cmd, p, "center")?, req_vec2(cmd, p, "end")?);
    let w = width(p, cmd)?;
    slot(s, p, cmd, c * 2.0 - e, e, w)
}

/// Arc slot around `o` (centre-line radius `r`) from angle `t0` sweeping `sweep` (> 0).
fn arc_slot(s: &mut Session, p: &Value, cmd: &str, o: Vec2, r: f64, t0: f64, sweep: f64, w: f64) -> Result<Value> {
    let h = w / 2.0;
    if h >= r {
        return Err(bad(cmd, "the slot is wider than its centre arc's diameter"));
    }
    if !(sweep > 1e-6 && sweep < std::f64::consts::TAU - 1e-6) {
        return Err(bad(cmd, "the centre arc must have a length"));
    }
    let t1 = t0 + sweep;
    let at = |t: f64, rr: f64| o + Vec2::from_angle(t) * rr;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let oc = sk.add_point(o, None)?;
        let ps = sk.add_point(at(t0, r), None)?;
        let pe = sk.add_point(at(t1, r), None)?;
        let (os, is) = (sk.add_point(at(t0, r + h), None)?, sk.add_point(at(t0, r - h), None)?);
        let (oe, ie) = (sk.add_point(at(t1, r + h), None)?, sk.add_point(at(t1, r - h), None)?);
        let outer = sk.add_arc(o, at(t0, r + h), at(t1, r + h), [Some(oc), Some(os), Some(oe)], None)?;
        let inner = sk.add_arc(o, at(t0, r - h), at(t1, r - h), [Some(oc), Some(is), Some(ie)], None)?;
        let cap_e = sk.add_arc(at(t1, r), at(t1, r + h), at(t1, r - h), [Some(pe), Some(oe), Some(ie)], None)?;
        let cap_s = sk.add_arc(at(t0, r), at(t0, r - h), at(t0, r + h), [Some(ps), Some(is), Some(os)], None)?;
        let centre = sk.add_arc(o, at(t0, r), at(t1, r), [Some(oc), Some(ps), Some(pe)], None)?;
        if let Some(c) = sk.curves.get_mut(centre) {
            c.construction = true;
        }
        let mut cons = Vec::new();
        for (a, b) in [(outer, cap_e), (inner, cap_e), (outer, cap_s), (inner, cap_s)] {
            cons.push(add_c(sk, ConstraintKind::Tangent { a, b })?);
        }
        cons.push(add_c(sk, ConstraintKind::Equal { a: cap_s, b: cap_e })?);
        Ok((ids_of(sk, &[outer, cap_e, inner, cap_s]), cons))
    })?;
    Ok(result(out, info))
}

fn arc_slot_three(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeArcSlotThreePoint";
    let (a, t, b) = (req_vec2(cmd, p, "start")?, req_vec2(cmd, p, "through")?, req_vec2(cmd, p, "end")?);
    let w = width(p, cmd)?;
    let (o, r) = circumcircle(a, t, b).ok_or_else(|| bad(cmd, "the points are collinear"))?;
    let ang = |q: Vec2| (q - o).angle();
    let span = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
    // Counter-clockwise from start must pass the through point; otherwise go from the end.
    let (t0, sweep) = if span(ang(a), ang(t)) < span(ang(a), ang(b)) { (ang(a), span(ang(a), ang(b))) } else { (ang(b), span(ang(b), ang(a))) };
    arc_slot(s, p, cmd, o, r, t0, sweep, w)
}

fn arc_slot_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeArcSlotCenterTwoPoint";
    let (o, a, b) = (req_vec2(cmd, p, "center")?, req_vec2(cmd, p, "start")?, req_vec2(cmd, p, "end")?);
    let w = width(p, cmd)?;
    let r = o.dist(a);
    if r < 1e-9 || o.dist(b) < 1e-9 {
        return Err(bad(cmd, "the points must differ from the centre"));
    }
    let t0 = (a - o).angle();
    let sweep = ((b - o).angle() - t0).rem_euclid(std::f64::consts::TAU);
    arc_slot(s, p, cmd, o, r, t0, sweep, w)
}

/// Solve moving only the points from `first_new` on (the geometry a tool just made), so the
/// existing drawing stays put when the new entities alone can satisfy the constraints. Falls
/// back to leaving everything to the normal solve.
pub(super) fn solve_new_only(sk: &mut Sketch, first_new: usize) {
    let mut trial = sk.clone();
    for (i, p) in trial.points.iter_mut().enumerate() {
        if i < first_new {
            p.fixed = true;
        }
    }
    if solvecraft_sketch::solve(&mut trial).ok() {
        for (p, t) in sk.points.iter_mut().zip(&trial.points).skip(first_new) {
            p.pos = t.pos;
        }
        for (c, t) in sk.curves.iter_mut().zip(&trial.curves) {
            if let (CurveKind::Circle { r, .. }, CurveKind::Circle { r: tr, .. }) = (&mut c.kind, &t.kind) {
                *r = *tr;
            }
        }
    }
}
