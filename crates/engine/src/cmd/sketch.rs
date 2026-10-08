//! Sketch commands: create/edit/finish sketches, draw curves, constraints and dimensions.

use serde_json::{Value, json};
use solvecraft_doc::{FeatureKind, PlaneRef};
use solvecraft_geom::{Plane, Vec2, Vec3};
use solvecraft_sketch::{ConstraintKind, CurveKind, Sketch, solve};

use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, expr, num, req_vec2, str_, string_list, vec2, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("SketchCreate", "Create Sketch", create_sketch)
        .at("SOLID", "CREATE")
        .icon("sketch")
        .params("plane: XY|XZ|YZ | {origin, x_dir, y_dir} | {face: [x,y,z]} (planar face at point), offset?: expr, name?"),
    CommandSpec::new("SketchActivate", "Edit Sketch", edit_sketch).icon("sketch").params("sketch: id or name"),
    CommandSpec::new("SketchStop", "Finish Sketch", finish_sketch).at("SKETCH", "FINISH SKETCH").icon("finish").enabled(in_sketch).noundo(),
    CommandSpec::new("DrawPolyline", "Line", draw_line)
        .at("SKETCH", "CREATE")
        .icon("line")
        .key("L")
        .enabled(in_sketch)
        .params("points: [[x,y] | \"l1.end\" | {at, ref}…], closed?: bool, construction?: bool, ids?: [..], infer?: bool (add horizontal/vertical)"),
    CommandSpec::new("ShapeRectangleTwoPoint", "2-Point Rectangle", rect_two_point)
        .at("SKETCH", "CREATE")
        .icon("rect")
        .key("R")
        .enabled(in_sketch)
        .params("p0, p1: opposite corners [x,y]; construction?"),
    CommandSpec::new("ShapeRectangleThreePoint", "3-Point Rectangle", rect_three_point)
        .at("SKETCH", "CREATE")
        .icon("rect3")
        .enabled(in_sketch)
        .params("p0, p1: first edge; p2: point on the opposite edge"),
    CommandSpec::new("ShapeRectangleCenter", "Center Rectangle", rect_center)
        .at("SKETCH", "CREATE")
        .icon("rect_center")
        .enabled(in_sketch)
        .params("center, corner"),
    CommandSpec::new("CircleCenterRadius", "Center Diameter Circle", circle_center)
        .at("SKETCH", "CREATE")
        .icon("circle")
        .key("C")
        .enabled(in_sketch)
        .params("center: [x,y] or point ref, radius | diameter: number"),
    CommandSpec::new("CircleTwoPoint", "2-Point Circle", circle_two)
        .at("SKETCH", "CREATE")
        .icon("circle2")
        .enabled(in_sketch)
        .params("p0, p1: diameter ends"),
    CommandSpec::new("CircleThreePoint", "3-Point Circle", circle_three)
        .at("SKETCH", "CREATE")
        .icon("circle3")
        .enabled(in_sketch)
        .params("p0, p1, p2"),
    CommandSpec::new("ArcThreePoint", "3-Point Arc", arc_three).at("SKETCH", "CREATE").icon("arc3").enabled(in_sketch).params("start, end, through"),
    CommandSpec::new("ArcCenterTwoPoint", "Center Point Arc", arc_center)
        .at("SKETCH", "CREATE")
        .icon("arc_center")
        .enabled(in_sketch)
        .params("center, start, end (counter-clockwise from start) | center, start, sweep: signed degrees"),
    CommandSpec::new("ShapePolygonInscribed", "Inscribed Polygon", polygon_inscribed)
        .at("SKETCH", "CREATE")
        .icon("polygon")
        .enabled(in_sketch)
        .params("center, radius (to vertices), sides, angle?: deg"),
    CommandSpec::new("ShapePolygonCircumscribed", "Circumscribed Polygon", polygon_circumscribed)
        .at("SKETCH", "CREATE")
        .icon("polygon")
        .enabled(in_sketch)
        .params("center, radius (to edge midpoints), sides, angle?: deg"),
    CommandSpec::new("ShapePolygonEdge", "Edge Polygon", polygon_edge)
        .at("SKETCH", "CREATE")
        .icon("polygon")
        .enabled(in_sketch)
        .params("p0, p1: one edge, sides"),
    CommandSpec::new("ShapeSlotCenterToCenter", "Center to Center Slot", slot_c2c)
        .at("SKETCH", "CREATE")
        .icon("slot")
        .enabled(in_sketch)
        .params("p0, p1: arc centres, width"),
    CommandSpec::new("ShapeSlotOverall", "Overall Slot", slot_overall)
        .at("SKETCH", "CREATE")
        .icon("slot")
        .enabled(in_sketch)
        .params("p0, p1: overall ends, width"),
    CommandSpec::new("DrawPoint", "Point", draw_point).at("SKETCH", "CREATE").icon("point").enabled(in_sketch).params("point: [x,y], id?"),
    CommandSpec::new("SketchDimension", "Sketch Dimension", dimension).at("SKETCH", "CREATE").icon("dimension").key("D").enabled(in_sketch).params(
        "entities: [refs], type?: auto|distance|horizontal|vertical|length|radius|diameter|angle, value?: number or expression (default: current)",
    ),
    CommandSpec::new("ConstraintHorizontalVertical", "Horizontal/Vertical", c_horizontal_vertical)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_hv")
        .enabled(in_sketch)
        .params("line: id | points: [a, b]; mode?: horizontal|vertical"),
    CommandSpec::new("ConstraintCoincident", "Coincident", c_coincident)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_coincident")
        .enabled(in_sketch)
        .params("a: point, b: point or curve"),
    CommandSpec::new("ConstraintTangent", "Tangent", c_tangent)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_tangent")
        .enabled(in_sketch)
        .params("a, b: curves"),
    CommandSpec::new("ConstraintEqual", "Equal", c_equal)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_equal")
        .enabled(in_sketch)
        .params("a, b: lines or circles/arcs"),
    CommandSpec::new("ConstraintParallel", "Parallel", c_parallel)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_parallel")
        .enabled(in_sketch)
        .params("a, b: lines"),
    CommandSpec::new("ConstraintPerpendicular", "Perpendicular", c_perpendicular)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_perpendicular")
        .enabled(in_sketch)
        .params("a, b: lines"),
    CommandSpec::new("ConstraintFix", "Fix/UnFix", c_fix)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_fix")
        .enabled(in_sketch)
        .params("entity: point or curve | entities: [points and curves]; fixed?: bool (default: fix them all unless all are fixed, then unfix)"),
    CommandSpec::new("ConstraintMidPoint", "MidPoint", c_midpoint)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_midpoint")
        .enabled(in_sketch)
        .params("point, line"),
    CommandSpec::new("ConstraintConcentric", "Concentric", c_concentric)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_concentric")
        .enabled(in_sketch)
        .params("a, b: circles/arcs"),
    CommandSpec::new("ConstraintCollinear", "Collinear", c_collinear)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_collinear")
        .enabled(in_sketch)
        .params("a, b: lines"),
    CommandSpec::new("ConstraintSymmetry", "Symmetry", c_symmetry)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_symmetry")
        .enabled(in_sketch)
        .params("a, b: points, line: symmetry line"),
    CommandSpec::new("sketch.construction", "Normal/Construction", construction)
        .at("SKETCH", "CREATE")
        .icon("construction")
        .key("X")
        .enabled(in_sketch)
        .params("curves: [ids], value?: bool (default toggles)"),
    CommandSpec::new("sketch.delete", "Delete Sketch Entities", sketch_delete)
        .enabled(in_sketch)
        .params("entities: [curve, point or constraint ids]"),
    CommandSpec::new("sketch.move_point", "Drag Sketch Point", move_point).enabled(in_sketch).params("point: ref, to: [x,y]"),
    CommandSpec::new("sketch.dimension_text", "Move Dimension Text", dimension_text)
        .enabled(in_sketch)
        .params("dimension: constraint id or parameter name; at: [x,y] text centre (sketch coordinates) | reset: true (default place)"),
    CommandSpec::new("sketch.solve", "Solve Sketch", solve_cmd)
        .enabled(in_sketch)
        .noundo()
        .params("reports the solver status and degrees of freedom"),
];

// ---------------------------------------------------------------------------------------------
// Helpers

/// Resolve the sketch a command acts on: `sketch` param (id or name) or the active sketch.
pub(super) fn target_sketch(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    if let Some(v) = p.get("sketch") {
        let key = match v {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err(bad(cmd, "`sketch` must be an id or a name")),
        };
        let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
        if !matches!(f.kind, FeatureKind::Sketch { .. }) {
            return Err(bad(cmd, format!("`{key}` is not a sketch")));
        }
        return Ok(f.id);
    }
    s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))
}

/// Edit a sketch: clone it, apply `f`, solve with the document's dimension values and store
/// it. With `strict`, an edit that makes the constraints unsolvable is rejected.
pub(super) fn edit<T>(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    strict: bool,
    f: impl FnOnce(&mut Sketch, &mut solvecraft_doc::Document) -> Result<T>,
) -> Result<(T, Value)> {
    let id = target_sketch(s, p, cmd)?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    super::sketch_project::refresh(s, &doc, id, &mut sk);
    let out = f(&mut sk, &mut doc)?;
    let (vals, _) = doc.param_values();
    doc.apply_dimension_values(&vals, &mut sk)?;
    let rep = solve(&mut sk);
    if strict && !rep.ok() {
        return Err(EngineError::Other(format!("that would over-constrain the sketch (conflicts with {})", rep.failing.join(", "))));
    }
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok((out, json!({"solved": rep.ok(), "dof": rep.dof})))
}

/// A point argument: `[x, y]` (new point), a point reference string, or `{at, ref}`.
#[derive(Clone)]
pub(super) enum PArg {
    At(Vec2),
    Ref(usize, Vec2),
}

impl PArg {
    pub(super) fn pos(&self) -> Vec2 {
        match self {
            PArg::At(p) | PArg::Ref(_, p) => *p,
        }
    }
    pub(super) fn idx(&self) -> Option<usize> {
        match self {
            PArg::Ref(i, _) => Some(*i),
            PArg::At(_) => None,
        }
    }
}

pub(super) fn parg(sk: &Sketch, v: &Value, cmd: &str) -> Result<PArg> {
    if let Some(p) = vec2(v) {
        return Ok(PArg::At(p));
    }
    let r = match v {
        Value::String(r) => Some(r.as_str()),
        Value::Object(o) => o.get("ref").and_then(Value::as_str),
        _ => None,
    };
    if let Some(r) = r {
        let i = sk.resolve_point(r).ok_or_else(|| bad(cmd, format!("unknown sketch point `{r}`")))?;
        return Ok(PArg::Ref(i, sk.point(i).unwrap_or_default()));
    }
    if let Some(at) = v.get("at").and_then(vec2) {
        return Ok(PArg::At(at));
    }
    Err(bad(cmd, "a point must be [x, y], a point id like \"l1.end\", or {at, ref}"))
}

pub(super) fn req_parg(sk: &Sketch, p: &Value, k: &str, cmd: &str) -> Result<PArg> {
    parg(sk, p.get(k).ok_or_else(|| bad(cmd, format!("missing `{k}`")))?, cmd)
}

pub(super) fn curve_ref(sk: &Sketch, v: Option<&Value>, cmd: &str, what: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{what}` must be a curve id")))?;
    sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown sketch curve `{id}`")))
}

pub(super) fn point_ref(sk: &Sketch, v: Option<&Value>, cmd: &str, what: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{what}` must be a point reference")))?;
    sk.resolve_point(id).ok_or_else(|| bad(cmd, format!("unknown sketch point `{id}`")))
}

pub(super) fn ids_of(sk: &Sketch, curves: &[usize]) -> Vec<String> {
    curves.iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect()
}

pub(super) fn add_c(sk: &mut Sketch, k: ConstraintKind) -> Result<String> {
    Ok(sk.add_constraint(k, None)?)
}

pub(super) fn mark_construction(sk: &mut Sketch, curves: &[usize], p: &Value) {
    if bool_(p, "construction").unwrap_or(false) {
        for c in curves {
            if let Some(cu) = sk.curves.get_mut(*c) {
                cu.construction = true;
            }
        }
    }
}

pub(super) fn result(sk_out: (Vec<String>, Vec<String>), info: Value) -> Value {
    json!({"curves": sk_out.0, "constraints": sk_out.1, "sketch": info})
}

// ---------------------------------------------------------------------------------------------
// Sketch lifecycle

pub(super) fn plane_ref(s: &Session, p: &Value, cmd: &str) -> Result<PlaneRef> {
    let base = match p.get("plane") {
        None => PlaneRef::Origin { name: "XY".into() },
        Some(Value::String(n)) => {
            if Plane::named(n).is_some() {
                PlaneRef::Origin { name: n.to_ascii_uppercase() }
            } else if matches!(s.doc.find_feature(n).map(|f| &f.kind), Some(FeatureKind::ConstructionPlane { .. })) {
                PlaneRef::Construction { name: n.clone() }
            } else {
                return Err(bad(cmd, format!("unknown plane `{n}` (XY, XZ, YZ or a construction plane)")));
            }
        }
        Some(v @ Value::Object(o)) => {
            if let Some(fp) = o.get("face").and_then(vec3) {
                // A planar face of a body at this point.
                let st = s.model.state();
                let mut found = None;
                for b in &st.bodies {
                    let tol = (b.body.size() * 1e-3).max(1e-3);
                    for f in b.body.faces(tol).unwrap_or_default() {
                        if let Some(n) = f.plane_normal
                            && (fp - f.centroid).dot(n).abs() < tol * 10.0
                            && b.body.tessellate(tol).map(|m| face_contains(&m, f.index, fp, tol * 10.0)).unwrap_or(false)
                        {
                            // Sketch origin: the model origin projected onto the face plane.
                            let d = (f.centroid + n * (fp - f.centroid).dot(n)).dot(n);
                            found = Plane::from_normal(n * d, n);
                        }
                    }
                }
                let pl = found.ok_or_else(|| bad(cmd, "no planar face at that point"))?;
                PlaneRef::Custom { plane: pl }
            } else {
                let origin = v.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
                let x = v.get("x_dir").and_then(vec3).ok_or_else(|| bad(cmd, "plane needs x_dir"))?;
                let y = v.get("y_dir").and_then(vec3).ok_or_else(|| bad(cmd, "plane needs y_dir"))?;
                PlaneRef::Custom { plane: Plane::new(origin, x, y).ok_or_else(|| bad(cmd, "degenerate plane"))? }
            }
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ or an object")),
    };
    Ok(match expr(p, "offset") {
        Some(d) => PlaneRef::Offset { base: Box::new(base), distance: d },
        None => base,
    })
}

/// Does triangle set of face `fi` contain point `p` (within `tol` of the face plane)?
fn face_contains(m: &solvecraft_geom::Mesh, fi: usize, p: Vec3, tol: f64) -> bool {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == fi).any(|(t, _)| {
        let Some([a, b, c]) = m.tri(t) else { return false };
        let n = (b - a).cross(c - a);
        let Some(nn) = n.normalized() else { return false };
        if (p - a).dot(nn).abs() > tol {
            return false;
        }
        let s1 = (b - a).cross(p - a).dot(n);
        let s2 = (c - b).cross(p - b).dot(n);
        let s3 = (a - c).cross(p - c).dot(n);
        (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0)
    })
}

fn create_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchCreate";
    let plane = plane_ref(s, p, cmd)?;
    // Validate the plane resolves.
    let (vals, _) = s.doc.param_values();
    s.doc.resolve_plane(&vals, &plane, 0)?;
    let name = str_(p, "name");
    let mut kind = FeatureKind::Sketch { plane, sketch: Sketch::new() };
    super::component::to_active_frame(s, &mut kind);
    let id = s.doc_mut().add_feature(kind, name)?;
    let comp = s.active_component;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = comp;
    }
    s.active_sketch = Some(id);
    let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    // Sketching on a face projects the face's edges (unless turned off).
    let face = p.get("plane").and_then(|v| v.get("face")).and_then(vec3);
    let mut link = None;
    if let Some(at) = face
        && bool_(p, "project_edges").unwrap_or(s.auto_project)
    {
        link = super::sketch_project::auto_project_face(s, id, at)?;
    }
    Ok(json!({"sketch": id, "name": name, "projected": link}))
}

fn edit_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let key = match p.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad("SketchActivate", "`sketch` must be an id or a name")),
    };
    let f = s.doc.find_feature(&key).ok_or_else(|| bad("SketchActivate", format!("no sketch `{key}`")))?;
    if !matches!(f.kind, FeatureKind::Sketch { .. }) {
        return Err(bad("SketchActivate", format!("`{key}` is not a sketch")));
    }
    let (id, name) = (f.id, f.name.clone());
    s.active_sketch = Some(id);
    s.revision += 1;
    Ok(json!({"sketch": id, "name": name}))
}

fn finish_sketch(s: &mut Session, _p: &Value) -> Result<Value> {
    let id = s.active_sketch.take();
    s.revision += 1;
    let profiles = id.and_then(|i| s.model.state().sketch(i).map(|ss| ss.profiles.len())).unwrap_or(0);
    Ok(json!({"sketch": id, "profiles": profiles}))
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn draw_line(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "DrawPolyline";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let list = p.get("points").and_then(Value::as_array).cloned().ok_or_else(|| bad(cmd, "`points` must be a list"))?;
    if list.len() < 2 || list.len() > 10_000 {
        return Err(bad(cmd, "a line needs 2 or more points"));
    }
    let closed = bool_(p, "closed").unwrap_or(false);
    let infer = bool_(p, "infer").unwrap_or(false);
    let ids = string_list(p, "ids");
    let ((curves, cons), info) = edit(s, p, cmd, false, |sk, _| {
        let args: Vec<PArg> = list.iter().map(|v| parg(sk, v, cmd)).collect::<Result<_>>()?;
        let n = args.len();
        let segs = if closed { n } else { n - 1 };
        let mut pts: Vec<Option<usize>> = args.iter().map(PArg::idx).collect();
        let mut curves = Vec::new();
        let mut cons = Vec::new();
        for i in 0..segs {
            let j = (i + 1) % n;
            let (Some(a), Some(b)) = (args.get(i), args.get(j)) else { continue };
            if a.pos().dist(b.pos()) < 1e-9 {
                return Err(bad(cmd, "zero-length line"));
            }
            let id = ids.get(i).map(String::as_str);
            let c = sk.add_line(a.pos(), b.pos(), pts.get(i).copied().flatten(), pts.get(j).copied().flatten(), id)?;
            if let Some(CurveKind::Line { a: pa, b: pb }) = sk.curves.get(c).map(|c| c.kind.clone()) {
                if let Some(slot) = pts.get_mut(i) {
                    *slot = Some(pa);
                }
                if let Some(slot) = pts.get_mut(j) {
                    *slot = Some(pb);
                }
            }
            if infer {
                let d = b.pos() - a.pos();
                let axis = if d.y.abs() <= d.len() * 1e-9 {
                    cons.push(add_c(sk, ConstraintKind::Horizontal { l: c })?);
                    true
                } else if d.x.abs() <= d.len() * 1e-9 {
                    cons.push(add_c(sk, ConstraintKind::Vertical { l: c })?);
                    true
                } else {
                    false
                };
                if let Some(k) = infer_at_start(sk, c, axis) {
                    cons.push(add_c(sk, k)?);
                }
            }
            curves.push(c);
        }
        mark_construction(sk, &curves, p);
        Ok((ids_of(sk, &curves), cons))
    })?;
    Ok(result((curves, cons), info))
}

/// A relation the new line `c` has with the curve it starts from (drawn exactly so): tangent
/// to an arc, or perpendicular to a line (parallel continuations are left alone). `axis`: the
/// line already got horizontal/vertical, so a perpendicular to an axis-aligned line would be
/// redundant.
fn infer_at_start(sk: &Sketch, c: usize, axis: bool) -> Option<ConstraintKind> {
    let CurveKind::Line { a, b } = sk.curves.get(c)?.kind else { return None };
    let (pa, pb) = (sk.point(a)?, sk.point(b)?);
    let u = (pb - pa).normalized()?;
    for (i, o) in sk.curves.iter().enumerate() {
        if i == c || !o.kind.uses(a) {
            continue;
        }
        match o.kind {
            CurveKind::Arc { c: cc, .. } => {
                let r = (pa - sk.point(cc)?).normalized()?;
                if r.dot(u).abs() < 1e-9 {
                    return Some(ConstraintKind::Tangent { a: i, b: c });
                }
            }
            CurveKind::Line { a: oa, b: ob } => {
                let v = (sk.point(ob)? - sk.point(oa)?).normalized()?;
                let o_axis = v.x.abs() < 1e-9 || v.y.abs() < 1e-9;
                if u.dot(v).abs() < 1e-9 && !(axis && o_axis) {
                    return Some(ConstraintKind::Perpendicular { a: i, b: c });
                }
            }
            _ => {}
        }
    }
    None
}

/// Closed polygon of lines through new or referenced corner points.
fn polygon_lines(sk: &mut Sketch, corners: &[Vec2]) -> Result<(Vec<usize>, Vec<usize>)> {
    let pts: Vec<usize> = corners.iter().map(|c| sk.add_point(*c, None)).collect::<std::result::Result<_, _>>()?;
    let n = pts.len();
    let mut lines = Vec::new();
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        lines.push(sk.add_line_pts(*a, *b, None)?);
    }
    Ok((lines, pts))
}

fn rect_constraints(sk: &mut Sketch, l: &[usize]) -> Result<Vec<String>> {
    let mut cons = Vec::new();
    if let [a, b, c, d] = l[..] {
        cons.push(add_c(sk, ConstraintKind::Horizontal { l: a })?);
        cons.push(add_c(sk, ConstraintKind::Horizontal { l: c })?);
        cons.push(add_c(sk, ConstraintKind::Vertical { l: b })?);
        cons.push(add_c(sk, ConstraintKind::Vertical { l: d })?);
    }
    Ok(cons)
}

fn rect_two_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeRectangleTwoPoint";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
        return Err(bad(cmd, "the corners must differ in x and y"));
    }
    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (l, _) = polygon_lines(sk, &[lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)])?;
        let cons = rect_constraints(sk, &l)?;
        mark_construction(sk, &l, p);
        Ok((ids_of(sk, &l), cons))
    })?;
    Ok(result(out, info))
}

fn rect_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeRectangleCenter";
    let (c, k) = (req_vec2(cmd, p, "center")?, req_vec2(cmd, p, "corner")?);
    let h = Vec2::new((k.x - c.x).abs(), (k.y - c.y).abs());
    if h.x < 1e-9 || h.y < 1e-9 {
        return Err(bad(cmd, "the corner must differ from the centre in x and y"));
    }
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (l, pts) = polygon_lines(sk, &[c - h, Vec2::new(c.x + h.x, c.y - h.y), c + h, Vec2::new(c.x - h.x, c.y + h.y)])?;
        let mut cons = rect_constraints(sk, &l)?;
        if let (Some(p0), Some(p2)) = (pts.first(), pts.get(2)) {
            let diag = sk.add_line_pts(*p0, *p2, None)?;
            if let Some(cu) = sk.curves.get_mut(diag) {
                cu.construction = true;
            }
            let cp = sk.add_point(c, None)?;
            cons.push(add_c(sk, ConstraintKind::Midpoint { p: cp, l: diag })?);
        }
        mark_construction(sk, &l, p);
        Ok((ids_of(sk, &l), cons))
    })?;
    Ok(result(out, info))
}

fn rect_three_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeRectangleThreePoint";
    let (a, b, c) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?, req_vec2(cmd, p, "p2")?);
    let d = (b - a).normalized().ok_or_else(|| bad(cmd, "p0 and p1 must differ"))?;
    let n = d.perp();
    let w = (c - a).dot(n);
    if w.abs() < 1e-9 {
        return Err(bad(cmd, "p2 must not be on the first edge"));
    }
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (l, _) = polygon_lines(sk, &[a, b, b + n * w, a + n * w])?;
        let mut cons = Vec::new();
        if let [l0, l1, l2, l3] = l[..] {
            cons.push(add_c(sk, ConstraintKind::Perpendicular { a: l0, b: l1 })?);
            cons.push(add_c(sk, ConstraintKind::Parallel { a: l0, b: l2 })?);
            cons.push(add_c(sk, ConstraintKind::Parallel { a: l1, b: l3 })?);
        }
        mark_construction(sk, &l, p);
        Ok((ids_of(sk, &l), cons))
    })?;
    Ok(result(out, info))
}

pub(super) fn radius_arg(p: &Value, cmd: &str) -> Result<f64> {
    let r = match (num(p, "radius"), num(p, "diameter")) {
        (Some(r), _) => r,
        (None, Some(d)) => d / 2.0,
        _ => return Err(bad(cmd, "needs `radius` or `diameter`")),
    };
    if r > 1e-9 { Ok(r) } else { Err(bad(cmd, "radius must be positive")) }
}

fn circle_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircleCenterRadius";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let r = radius_arg(p, cmd)?;
    let id = str_(p, "id").map(str::to_string);
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let c = req_parg(sk, p, "center", cmd)?;
        let ci = sk.add_circle(c.pos(), r, c.idx(), id.as_deref())?;
        mark_construction(sk, &[ci], p);
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn circle_two(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircleTwoPoint";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let r = a.dist(b) / 2.0;
    if r < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let ci = sk.add_circle((a + b) * 0.5, r, None, None)?;
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

/// Circle through three points.
pub(super) fn circumcircle(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let o = Vec2::new(ux, uy);
    let r = o.dist(a);
    (o.is_finite() && r.is_finite() && r < 1e8).then_some((o, r))
}

fn circle_three(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "CircleThreePoint";
    let (a, b, c) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?, req_vec2(cmd, p, "p2")?);
    let (o, r) = circumcircle(a, b, c).ok_or_else(|| bad(cmd, "the points are collinear"))?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let ci = sk.add_circle(o, r, None, None)?;
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn arc_three(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ArcThreePoint";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let id = str_(p, "id").map(str::to_string);
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (a, b) = (req_parg(sk, p, "start", cmd)?, req_parg(sk, p, "end", cmd)?);
        let t = req_vec2(cmd, p, "through")?;
        let (o, _) = circumcircle(a.pos(), t, b.pos()).ok_or_else(|| bad(cmd, "the points are collinear"))?;
        // Counter-clockwise from start to end must pass the through point; otherwise swap.
        let ang = |q: Vec2| (q - o).angle();
        let ccw_span = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        let ccw = ccw_span(ang(a.pos()), ang(t)) < ccw_span(ang(a.pos()), ang(b.pos()));
        let (s0, e0) = if ccw { (a, b) } else { (b, a) };
        let ci = sk.add_arc(o, s0.pos(), e0.pos(), [None, s0.idx(), e0.idx()], id.as_deref())?;
        if let Some(c) = sk.curves.get_mut(ci) {
            c.reversed = !ccw;
        }
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn arc_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ArcCenterTwoPoint";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let id = str_(p, "id").map(str::to_string);
    let sweep = num(p, "sweep");
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let c = req_parg(sk, p, "center", cmd)?;
        let a = req_parg(sk, p, "start", cmd)?;
        if let Some(sw) = sweep {
            // Start point and signed sweep (degrees); clockwise arcs keep `<id>.start` at the start.
            if !(sw.abs() > 1e-9 && sw.abs() < 360.0) {
                return Err(bad(cmd, "`sweep` must be between -360 and 360 degrees (not 0)"));
            }
            let end = c.pos() + Vec2::from_angle((a.pos() - c.pos()).angle() + sw.to_radians()) * a.pos().dist(c.pos());
            let cid = match &id {
                Some(i) => i.clone(),
                None => sk.fresh("a"),
            };
            let ai = match a.idx() {
                Some(i) => i,
                None => sk.add_point(a.pos(), Some(&format!("{cid}.start")))?,
            };
            let bi = sk.add_point(end, Some(&format!("{cid}.end")))?;
            let ci = if sw > 0.0 {
                sk.add_arc(c.pos(), a.pos(), end, [c.idx(), Some(ai), Some(bi)], Some(&cid))?
            } else {
                let ci = sk.add_arc(c.pos(), end, a.pos(), [c.idx(), Some(bi), Some(ai)], Some(&cid))?;
                if let Some(cu) = sk.curves.get_mut(ci) {
                    cu.reversed = true;
                }
                ci
            };
            return Ok((ids_of(sk, &[ci]), Vec::new()));
        }
        let b = req_parg(sk, p, "end", cmd)?;
        let ci = sk.add_arc(c.pos(), a.pos(), b.pos(), [c.idx(), a.idx(), b.idx()], id.as_deref())?;
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn polygon(s: &mut Session, p: &Value, cmd: &str, center: Vec2, r_vertex: f64, start: f64, n: usize, inscribed: bool) -> Result<Value> {
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let corners: Vec<Vec2> = (0..n).map(|i| center + Vec2::from_angle(start + std::f64::consts::TAU * i as f64 / n as f64) * r_vertex).collect();
        let (l, pts) = polygon_lines(sk, &corners)?;
        let mut cons = Vec::new();
        for w in l.windows(2) {
            if let [a, b] = w {
                cons.push(add_c(sk, ConstraintKind::Equal { a: *a, b: *b })?);
            }
        }
        // Construction circle tying the polygon to its centre.
        let rc = if inscribed { r_vertex } else { r_vertex * (std::f64::consts::PI / n as f64).cos() };
        let circ = sk.add_circle(center, rc, None, None)?;
        if let Some(cu) = sk.curves.get_mut(circ) {
            cu.construction = true;
        }
        if inscribed {
            for pt in &pts {
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: *pt, c: circ })?);
            }
        } else {
            for li in &l {
                cons.push(add_c(sk, ConstraintKind::Tangent { a: *li, b: circ })?);
            }
        }
        Ok((ids_of(sk, &l), cons))
    })?;
    Ok(result(out, info))
}

fn sides(p: &Value, cmd: &str) -> Result<usize> {
    let n = num(p, "sides").unwrap_or(6.0);
    if !(3.0..=200.0).contains(&n) {
        return Err(bad(cmd, "sides must be 3…200"));
    }
    Ok(n as usize)
}

fn polygon_inscribed(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapePolygonInscribed";
    let c = req_vec2(cmd, p, "center")?;
    let r = radius_arg(p, cmd)?;
    let n = sides(p, cmd)?;
    let a = num(p, "angle").unwrap_or(90.0).to_radians();
    polygon(s, p, cmd, c, r, a, n, true)
}

fn polygon_circumscribed(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapePolygonCircumscribed";
    let c = req_vec2(cmd, p, "center")?;
    let r = radius_arg(p, cmd)?;
    let n = sides(p, cmd)?;
    let a = num(p, "angle").unwrap_or(90.0).to_radians();
    polygon(s, p, cmd, c, r / (std::f64::consts::PI / n as f64).cos(), a + std::f64::consts::PI / n as f64, n, false)
}

fn polygon_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapePolygonEdge";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let n = sides(p, cmd)?;
    let e = a.dist(b);
    if e < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let rv = e / (2.0 * (std::f64::consts::PI / n as f64).sin());
    let apothem = rv * (std::f64::consts::PI / n as f64).cos();
    let mid = (a + b) * 0.5;
    let dir = (b - a).normalized().unwrap_or(Vec2::X);
    let center = mid + dir.perp() * apothem;
    let start = (a - center).angle();
    polygon(s, p, cmd, center, rv, start, n, true)
}

pub(super) fn slot(s: &mut Session, p: &Value, cmd: &str, c0: Vec2, c1: Vec2, w: f64) -> Result<Value> {
    let h = w / 2.0;
    let d = (c1 - c0).normalized().ok_or_else(|| bad(cmd, "the slot ends must differ"))?;
    let n = d.perp();
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let pts: Vec<usize> =
            [c0 - n * h, c1 - n * h, c1 + n * h, c0 + n * h].iter().map(|q| sk.add_point(*q, None)).collect::<std::result::Result<_, _>>()?;
        let (p0, p1, p2, p3) = (pts[0], pts[1], pts[2], pts[3]);
        let l1 = sk.add_line_pts(p0, p1, None)?;
        let a1 = sk.add_arc(c1, c1 - n * h, c1 + n * h, [None, Some(p1), Some(p2)], None)?;
        let l2 = sk.add_line_pts(p2, p3, None)?;
        let a0 = sk.add_arc(c0, c0 + n * h, c0 - n * h, [None, Some(p3), Some(p0)], None)?;
        let mut cons = Vec::new();
        for (l, a) in [(l1, a1), (l2, a1), (l1, a0), (l2, a0)] {
            cons.push(add_c(sk, ConstraintKind::Tangent { a: l, b: a })?);
        }
        cons.push(add_c(sk, ConstraintKind::Equal { a: a0, b: a1 })?);
        Ok((ids_of(sk, &[l1, a1, l2, a0]), cons))
    })?;
    Ok(result(out, info))
}

fn slot_c2c(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeSlotCenterToCenter";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let w = num(p, "width").filter(|w| *w > 1e-9).ok_or_else(|| bad(cmd, "`width` must be positive"))?;
    slot(s, p, cmd, a, b, w)
}

fn slot_overall(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ShapeSlotOverall";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let w = num(p, "width").filter(|w| *w > 1e-9).ok_or_else(|| bad(cmd, "`width` must be positive"))?;
    let d = (b - a).normalized().ok_or_else(|| bad(cmd, "the slot ends must differ"))?;
    if a.dist(b) <= w {
        return Err(bad(cmd, "the slot must be longer than it is wide"));
    }
    slot(s, p, cmd, a + d * (w / 2.0), b - d * (w / 2.0), w)
}

fn draw_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "DrawPoint";
    let at = req_vec2(cmd, p, "point")?;
    let id = str_(p, "id").map(str::to_string);
    let (pid, info) = edit(s, p, cmd, false, |sk, _| {
        let i = sk.add_point(at, id.as_deref())?;
        Ok(sk.points.get(i).map(|q| q.id.clone()).unwrap_or_default())
    })?;
    Ok(json!({"point": pid, "sketch": info}))
}

// ---------------------------------------------------------------------------------------------
// Constraints

fn constrain(s: &mut Session, p: &Value, cmd: &str, f: impl FnOnce(&Sketch) -> Result<ConstraintKind>) -> Result<Value> {
    let mut before = None;
    let (id, info) = edit(s, p, cmd, true, |sk, _| {
        let k = f(sk)?;
        before = Some(dof_now(sk));
        add_c(sk, k)
    })?;
    reject_redundant(before, &info, cmd)?;
    Ok(json!({"constraint": id, "sketch": info}))
}

/// Degrees of freedom of a sketch as it is.
pub(super) fn dof_now(sk: &Sketch) -> usize {
    solve(&mut sk.clone()).dof
}

/// A constraint that removes no degree of freedom over-constrains the sketch (Fusion refuses
/// it); the caller's edit is rolled back by returning the error.
pub(super) fn reject_redundant(before: Option<usize>, info: &Value, cmd: &str) -> Result<()> {
    let after = info.get("dof").and_then(Value::as_u64).map(|d| d as usize);
    if let (Some(b), Some(a)) = (before, after)
        && a >= b
    {
        return Err(bad(cmd, "that would over-constrain the sketch (it is already determined by other constraints)"));
    }
    Ok(())
}

fn c_horizontal_vertical(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintHorizontalVertical";
    let mode = str_(p, "mode").map(str::to_ascii_lowercase);
    constrain(s, p, cmd, |sk| {
        if let Some(pts) = p.get("points").and_then(Value::as_array) {
            let a = point_ref(sk, pts.first(), cmd, "points[0]")?;
            let b = point_ref(sk, pts.get(1), cmd, "points[1]")?;
            let d = sk.point(b).unwrap_or_default() - sk.point(a).unwrap_or_default();
            let horiz = match mode.as_deref() {
                Some("horizontal") => true,
                Some("vertical") => false,
                _ => d.x.abs() >= d.y.abs(),
            };
            return Ok(if horiz { ConstraintKind::HorizontalPoints { p: a, q: b } } else { ConstraintKind::VerticalPoints { p: a, q: b } });
        }
        let l = curve_ref(sk, p.get("line"), cmd, "line")?;
        let (a, b) = match sk.curves.get(l).map(|c| &c.kind) {
            Some(CurveKind::Line { a, b }) => (*a, *b),
            _ => return Err(bad(cmd, "`line` must be a line")),
        };
        let d = sk.point(b).unwrap_or_default() - sk.point(a).unwrap_or_default();
        let horiz = match mode.as_deref() {
            Some("horizontal") => true,
            Some("vertical") => false,
            _ => d.x.abs() >= d.y.abs(),
        };
        Ok(if horiz { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } })
    })
}

fn c_coincident(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintCoincident";
    constrain(s, p, cmd, |sk| {
        let a = point_ref(sk, p.get("a"), cmd, "a")?;
        let bref = p.get("b").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`b` must be a point or curve"))?;
        if let Some(b) = sk.resolve_point(bref) {
            return Ok(ConstraintKind::Coincident { p: a, q: b });
        }
        let c = sk.curve_index(bref).ok_or_else(|| bad(cmd, format!("unknown entity `{bref}`")))?;
        Ok(ConstraintKind::PointOnCurve { p: a, c })
    })
}

fn two_curves(sk: &Sketch, p: &Value, cmd: &str) -> Result<(usize, usize)> {
    Ok((curve_ref(sk, p.get("a"), cmd, "a")?, curve_ref(sk, p.get("b"), cmd, "b")?))
}

fn c_tangent(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintTangent", |sk| two_curves(sk, p, "ConstraintTangent").map(|(a, b)| ConstraintKind::Tangent { a, b }))
}
fn c_equal(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintEqual", |sk| two_curves(sk, p, "ConstraintEqual").map(|(a, b)| ConstraintKind::Equal { a, b }))
}
fn c_parallel(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintParallel", |sk| two_curves(sk, p, "ConstraintParallel").map(|(a, b)| ConstraintKind::Parallel { a, b }))
}
fn c_perpendicular(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintPerpendicular", |sk| two_curves(sk, p, "ConstraintPerpendicular").map(|(a, b)| ConstraintKind::Perpendicular { a, b }))
}
fn c_concentric(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintConcentric", |sk| two_curves(sk, p, "ConstraintConcentric").map(|(a, b)| ConstraintKind::Concentric { a, b }))
}
fn c_collinear(s: &mut Session, p: &Value) -> Result<Value> {
    constrain(s, p, "ConstraintCollinear", |sk| two_curves(sk, p, "ConstraintCollinear").map(|(a, b)| ConstraintKind::Collinear { a, b }))
}
fn c_midpoint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintMidPoint";
    constrain(s, p, cmd, |sk| {
        Ok(ConstraintKind::Midpoint { p: point_ref(sk, p.get("point"), cmd, "point")?, l: curve_ref(sk, p.get("line"), cmd, "line")? })
    })
}
fn c_symmetry(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintSymmetry";
    let (ids, info) = edit(s, p, cmd, true, |sk, _| {
        let l = curve_ref(sk, p.get("line"), cmd, "line")?;
        let ra = p.get("a").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`a` must be a point or curve"))?;
        let rb = p.get("b").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`b` must be a point or curve"))?;
        let mut out = Vec::new();
        match (sk.resolve_point(ra), sk.resolve_point(rb)) {
            (Some(a), Some(b)) => out.push(add_c(sk, ConstraintKind::Symmetric { p: a, q: b, l })?),
            _ => {
                // Two curves: circles/arcs mirror their centres and keep equal radii; lines mirror
                // their end points.
                let ca = sk.curve_index(ra).ok_or_else(|| bad(cmd, format!("unknown entity `{ra}`")))?;
                let cb = sk.curve_index(rb).ok_or_else(|| bad(cmd, format!("unknown entity `{rb}`")))?;
                match (sk.curves.get(ca).map(|c| c.kind.clone()), sk.curves.get(cb).map(|c| c.kind.clone())) {
                    (Some(CurveKind::Line { a: a0, b: a1 }), Some(CurveKind::Line { a: b0, b: b1 })) => {
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: a0, q: b0, l })?);
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: a1, q: b1, l })?);
                    }
                    (Some(_), Some(_)) => {
                        let pa = round_center(sk, ca).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                        let pb = round_center(sk, cb).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: pa, q: pb, l })?);
                        out.push(add_c(sk, ConstraintKind::Equal { a: ca, b: cb })?);
                    }
                    _ => return Err(bad(cmd, "symmetry needs two points or two curves")),
                }
            }
        }
        Ok(out)
    })?;
    Ok(json!({"constraints": ids, "sketch": info}))
}

fn round_center(sk: &Sketch, c: usize) -> Option<usize> {
    match sk.curves.get(c)?.kind {
        CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => Some(c),
        _ => None,
    }
}

fn c_fix(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintFix";
    let want = bool_(p, "fixed");
    let mut refs = string_list(p, "entities");
    if let Some(e) = str_(p, "entity") {
        refs.insert(0, e.to_string());
    }
    if refs.is_empty() {
        return Err(bad(cmd, "`entity` or `entities` must name sketch points or curves"));
    }
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        // Points and curves; a point is fixed by a Fix constraint, a curve by its flag (which
        // also holds its radius). Projected geometry is always fixed.
        let mut pts = Vec::new();
        let mut curves = Vec::new();
        for r in &refs {
            if let Some(i) = sk.resolve_point(r) {
                pts.push(i);
            } else {
                curves.push(sk.curve_index(r).ok_or_else(|| bad(cmd, format!("unknown entity `{r}`")))?);
            }
        }
        let all_fixed = pts.iter().all(|q| *q == 0 || sk.point_locked(*q)) && curves.iter().all(|c| sk.curve_locked(*c));
        let fix = want.unwrap_or(!all_fixed);
        let mut n = 0;
        for q in pts {
            let own = sk.points.get(q).is_some_and(|x| x.fixed || x.link.is_some());
            if q == 0 || own {
                continue;
            }
            let has = sk.constraints.iter().any(|c| c.kind == ConstraintKind::Fix { p: q });
            if fix && !has {
                add_c(sk, ConstraintKind::Fix { p: q })?;
                n += 1;
            } else if !fix && has {
                sk.constraints.retain(|c| c.kind != ConstraintKind::Fix { p: q });
                n += 1;
            }
        }
        for c in curves {
            let Some(cu) = sk.curves.get_mut(c) else { continue };
            if cu.link.is_some() {
                continue;
            }
            cu.fixed = fix;
            n += 1;
            if !fix {
                let ids = cu.kind.point_ids();
                sk.constraints.retain(|k| !matches!(k.kind, ConstraintKind::Fix { p } if ids.contains(&p)));
            }
        }
        Ok(json!({"fixed": fix, "changed": n}))
    })?;
    Ok(json!({"result": n, "sketch": info}))
}

// ---------------------------------------------------------------------------------------------
// Dimensions

pub(super) fn line_pts(sk: &Sketch, l: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(l)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

fn dimension(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchDimension";
    let ents = string_list(p, "entities");
    if ents.is_empty() || ents.len() > 2 {
        return Err(bad(cmd, "`entities` must list one or two sketch entities"));
    }
    let ty = str_(p, "type").unwrap_or("auto").to_ascii_lowercase();
    let ty = match (ty.as_str(), str_(p, "orientation").map(str::to_ascii_lowercase).as_deref()) {
        ("distance" | "auto", Some("horizontal")) => "horizontal".to_string(),
        ("distance" | "auto", Some("vertical")) => "vertical".to_string(),
        ("offset", _) => "distance".to_string(),
        _ => ty,
    };
    let value = expr(p, "value");
    let text_at = p.get("text_at").and_then(vec2);
    let driven = bool_(p, "driven").unwrap_or(false);
    let mut before = None;
    let ((param, kind_name, current), info) = edit(s, p, cmd, true, |sk, doc| {
        before = Some(dof_now(sk));
        let ent = |r: &str| -> Result<(Option<usize>, Option<usize>)> {
            if let Some(c) = sk.curve_index(r) {
                return Ok((None, Some(c)));
            }
            if let Some(q) = sk.resolve_point(r) {
                return Ok((Some(q), None));
            }
            Err(bad(cmd, format!("unknown sketch entity `{r}`")))
        };
        let e0 = ent(ents.first().map(String::as_str).unwrap_or(""))?;
        let e1 = match ents.get(1) {
            Some(r) => Some(ent(r)?),
            None => None,
        };
        let is_line = |c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Line { .. }));
        let is_circle = |c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Circle { .. }));
        let (k, cur): (ConstraintKind, f64) = match (e0, e1) {
            ((None, Some(c)), None) if is_line(c) => {
                let (a, b) = line_pts(sk, c).unwrap_or_default();
                match ty.as_str() {
                    "horizontal" => {
                        (ConstraintKind::DistanceX { p: line_ends(sk, c).0, q: line_ends(sk, c).1, value: (b.x - a.x).abs() }, (b.x - a.x).abs())
                    }
                    "vertical" => {
                        (ConstraintKind::DistanceY { p: line_ends(sk, c).0, q: line_ends(sk, c).1, value: (b.y - a.y).abs() }, (b.y - a.y).abs())
                    }
                    _ => (ConstraintKind::Length { l: c, value: a.dist(b) }, a.dist(b)),
                }
            }
            ((None, Some(c)), None) if ty == "arc_length" || ty == "arclength" => {
                let len = arc_len(sk, c).ok_or_else(|| bad(cmd, "arc length needs an arc"))?;
                (ConstraintKind::ArcLength { c, value: len }, len)
            }
            ((None, Some(c)), None) if matches!(sk.curves.get(c).map(|x| &x.kind), Some(CurveKind::Ellipse { .. })) => {
                // Ellipse: the major radius is the centre–axis-end distance, the minor its own.
                let Some(CurveKind::Ellipse { c: cp, m, r }) = sk.curves.get(c).map(|x| x.kind.clone()) else {
                    return Err(bad(cmd, "ellipse"));
                };
                if ty == "major" {
                    let d = sk.point(cp).zip(sk.point(m)).map(|(a, b)| a.dist(b)).unwrap_or(0.0);
                    (ConstraintKind::Distance { p: cp, q: m, value: d }, d)
                } else {
                    (ConstraintKind::Radius { c, value: r }, r)
                }
            }
            ((None, Some(c)), None) => {
                let r = sk.radius(c).ok_or_else(|| bad(cmd, "cannot dimension that curve"))?;
                let diameter = ty == "diameter" || (ty != "radius" && is_circle(c));
                if diameter { (ConstraintKind::Diameter { c, value: 2.0 * r }, 2.0 * r) } else { (ConstraintKind::Radius { c, value: r }, r) }
            }
            ((Some(a), None), Some((Some(b), None))) => {
                let (pa, pb) = (sk.point(a).unwrap_or_default(), sk.point(b).unwrap_or_default());
                match ty.as_str() {
                    "horizontal" => (ConstraintKind::DistanceX { p: a, q: b, value: (pb.x - pa.x).abs() }, (pb.x - pa.x).abs()),
                    "vertical" => (ConstraintKind::DistanceY { p: a, q: b, value: (pb.y - pa.y).abs() }, (pb.y - pa.y).abs()),
                    _ => (ConstraintKind::Distance { p: a, q: b, value: pa.dist(pb) }, pa.dist(pb)),
                }
            }
            ((Some(q), None), Some((None, Some(l)))) | ((None, Some(l)), Some((Some(q), None))) if is_line(l) => {
                let (a, b) = line_pts(sk, l).unwrap_or_default();
                let pq = sk.point(q).unwrap_or_default();
                let d = (b - a).normalized().map(|d| d.cross(pq - a).abs()).unwrap_or(0.0);
                if diameter_about(sk, l, &ty) {
                    (ConstraintKind::LinearDiameter { p: q, l, value: 2.0 * d }, 2.0 * d)
                } else {
                    (ConstraintKind::PointLineDistance { p: q, l, value: d }, d)
                }
            }
            ((None, Some(a)), Some((None, Some(b)))) if is_line(a) && is_line(b) => {
                let (a0, a1) = line_pts(sk, a).unwrap_or_default();
                let (b0, b1) = line_pts(sk, b).unwrap_or_default();
                let (da, db) = (a1 - a0, b1 - b0);
                if da.cross(db).abs() < 1e-9 * da.len() * db.len() && ty != "angle" {
                    // Parallel lines: distance between them (a diameter about a centerline).
                    let (a, b, b0) = if sk.curves.get(a).is_some_and(|c| c.centerline) { (a, b, b0) } else { (b, a, a0) };
                    let (bq, _) = line_ends(sk, b);
                    let (a0, a1) = line_pts(sk, a).unwrap_or_default();
                    let d = (a1 - a0).normalized().map(|d| d.cross(b0 - a0).abs()).unwrap_or(0.0);
                    if diameter_about(sk, a, &ty) {
                        (ConstraintKind::LinearDiameter { p: bq, l: a, value: 2.0 * d }, 2.0 * d)
                    } else {
                        (ConstraintKind::PointLineDistance { p: bq, l: a, value: d }, d)
                    }
                } else {
                    match text_at {
                        Some(t) => angle_by_sector(a, b, (a0, a1), (b0, b1), t).ok_or_else(|| bad(cmd, "cannot place that angle"))?,
                        None => {
                            let mut ang = da.cross(db).atan2(da.dot(db));
                            let (mut ka, mut kb) = (a, b);
                            if ang < 0.0 {
                                ang = -ang;
                                std::mem::swap(&mut ka, &mut kb);
                            }
                            (ConstraintKind::Angle { a: ka, b: kb, value: ang, flip: false }, ang)
                        }
                    }
                }
            }
            _ => return Err(bad(cmd, "cannot dimension that combination of entities")),
        };
        let is_angle = k.is_angle();
        if driven {
            // A reference dimension: measures, no parameter.
            let name = k.name();
            let id = sk.add_constraint(k, None)?;
            place_text(sk, &id, text_at);
            if let Some(c) = sk.constraints.iter_mut().find(|c| c.id == id) {
                c.driven = true;
            }
            before = None;
            return Ok((String::new(), name, cur));
        }
        let (unit, default_expr) =
            if is_angle { ("deg", format!("{} deg", round6(cur.to_degrees()))) } else { ("mm", format!("{} mm", round6(cur))) };
        let e = value.clone().unwrap_or(default_expr);
        let pname = doc.new_model_param(&e, unit);
        // The value must evaluate.
        let (vals, errs) = doc.param_values();
        if let Some(err) = errs.get(&pname) {
            return Err(bad(cmd, format!("value: {err}")));
        }
        let _ = vals;
        let name = k.name();
        let id = sk.add_constraint(k, Some(pname.clone()))?;
        place_text(sk, &id, text_at);
        Ok((pname, name, cur))
    })?;
    if driven {
        return Ok(json!({"param": Value::Null, "driven": true, "type": kind_name, "measured": current, "sketch": info}));
    }
    if reject_redundant(before, &info, cmd).is_err() {
        return Err(bad(cmd, "that dimension would over-constrain the sketch; add it as a driven (reference) dimension with driven: true"));
    }
    let v = s.doc.param(&param).map(|p| p.expr.clone()).unwrap_or_default();
    Ok(json!({"param": param, "type": kind_name, "expression": v, "measured": current, "sketch": info}))
}

/// Keep a dimension's text where it was placed (`at`, sketch coordinates), relative to the
/// dimension so it follows the geometry.
fn place_text(sk: &mut Sketch, id: &str, at: Option<Vec2>) {
    let Some(i) = sk.constraints.iter().position(|c| c.id == id) else { return };
    let text = match (at, sk.constraints.get(i).and_then(|c| solvecraft_sketch::dim_frame(sk, &c.kind))) {
        (Some(at), Some(f)) => Some(solvecraft_sketch::encode_text(&f, at)),
        _ => None,
    };
    if let Some(c) = sk.constraints.get_mut(i) {
        c.text = text.filter(|t| t.is_finite() && t.x.abs() < 1e9 && t.y.abs() < 1e9);
    }
}

fn dimension_text(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.dimension_text";
    let key = str_(p, "dimension").ok_or_else(|| bad(cmd, "`dimension` must be a dimension id or parameter name"))?.to_string();
    let at = if bool_(p, "reset").unwrap_or(false) { None } else { Some(req_vec2(cmd, p, "at")?) };
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        let c = sk
            .constraints
            .iter()
            .find(|c| c.kind.is_dimension() && (c.id == key || c.param.as_deref() == Some(key.as_str())))
            .ok_or_else(|| bad(cmd, format!("no dimension `{key}` in this sketch")))?;
        let id = c.id.clone();
        place_text(sk, &id, at);
        Ok(id)
    })?;
    Ok(json!({"dimension": id, "sketch": info}))
}

/// Dimension a point (or a parallel line) to line `l` as a diameter: when `l` is a centerline
/// (unless a plain distance was asked for) or when a diameter was asked for.
fn diameter_about(sk: &Sketch, l: usize, ty: &str) -> bool {
    ty == "diameter" || (ty == "auto" && sk.curves.get(l).is_some_and(|c| c.centerline))
}

fn arc_len(sk: &Sketch, c: usize) -> Option<f64> {
    match sk.segs(c).first()? {
        solvecraft_geom::Seg2::Arc { radius, sweep, .. } if matches!(sk.curves.get(c)?.kind, CurveKind::Arc { .. }) => Some(radius * sweep),
        _ => None,
    }
}

/// The angle dimension between two lines whose sector contains `t` (the text position): returns
/// the constraint and the current value of that sector angle.
fn angle_by_sector(a: usize, b: usize, (a0, a1): (Vec2, Vec2), (b0, b1): (Vec2, Vec2), t: Vec2) -> Option<(ConstraintKind, f64)> {
    let (da, db) = ((a1 - a0).normalized()?, (b1 - b0).normalized()?);
    let den = da.cross(db);
    if den.abs() < 1e-12 {
        return None;
    }
    let s = (b0 - a0).cross(db) / den;
    let x = a0 + da * s;
    let ta = (t - x).angle();
    // The four rays from the intersection, with their line and direction sign.
    let mut rays: Vec<(f64, usize, bool)> = vec![(da.angle(), a, true), ((-da).angle(), a, false), (db.angle(), b, true), ((-db).angle(), b, false)];
    rays.sort_by(|p, q| p.0.total_cmp(&q.0));
    let tau = std::f64::consts::TAU;
    for k in 0..4 {
        let (s0, l0, p0) = rays[k];
        let (s1, l1, p1) = rays[(k + 1) % 4];
        let span = (s1 - s0).rem_euclid(tau);
        let off = (ta - s0).rem_euclid(tau);
        if off <= span && l0 != l1 {
            return Some((ConstraintKind::Angle { a: l0, b: l1, value: span, flip: p0 != p1 }, span));
        }
    }
    None
}

pub(super) fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn line_ends(sk: &Sketch, l: usize) -> (usize, usize) {
    match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => (*a, *b),
        _ => (0, 0),
    }
}

// ---------------------------------------------------------------------------------------------
// Editing

fn construction(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.construction";
    let ids = string_list(p, "curves");
    if ids.is_empty() {
        return Err(bad(cmd, "`curves` must list curve ids"));
    }
    let want = bool_(p, "value");
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        let mut n = 0;
        for id in &ids {
            let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.construction = want.unwrap_or(!cu.construction);
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"changed": n, "sketch": info}))
}

fn sketch_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.delete";
    let ids = string_list(p, "entities");
    if ids.is_empty() {
        return Err(bad(cmd, "`entities` must list sketch entity ids"));
    }
    let (n, info) = edit(s, p, cmd, false, |sk, doc| {
        let mut curves = Vec::new();
        let mut points = Vec::new();
        let mut cons = Vec::new();
        for id in &ids {
            if let Some(c) = sk.curve_index(id) {
                curves.push(c);
            } else if let Some(q) = sk.point_index(id) {
                if q == 0 {
                    return Err(bad(cmd, "the sketch origin cannot be deleted"));
                }
                points.push(q);
            } else if sk.constraints.iter().any(|c| &c.id == id) {
                cons.push(id.clone());
            } else if sk.wire_index(id).is_some() {
                sk.remove_wires(std::slice::from_ref(id));
            } else if sk.link(id).is_some() {
                curves.extend(sk.link_curves(id));
                points.extend(sk.link_points(id));
            } else {
                return Err(bad(cmd, format!("unknown sketch entity `{id}`")));
            }
        }
        sk.constraints.retain(|c| !cons.contains(&c.id));
        let pids: Vec<String> = points.iter().filter_map(|q| sk.points.get(*q).map(|x| x.id.clone())).collect();
        sk.remove_curves(&curves);
        let pidx: Vec<usize> = pids.iter().filter_map(|id| sk.point_index(id)).collect();
        sk.remove_points(&pidx);
        doc.prune_model_params();
        Ok(ids.len())
    })?;
    // Dimension parameters may have been dropped; prune again with the stored sketch.
    s.doc_mut().prune_model_params();
    Ok(json!({"deleted": n, "sketch": info}))
}

fn move_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move_point";
    let to = req_vec2(cmd, p, "to")?;
    let (_, info) = edit(s, p, cmd, false, |sk, _| {
        let q = point_ref(sk, p.get("point"), cmd, "point")?;
        let locked = sk.point_locked(q);
        if let Some(pt) = sk.points.get_mut(q)
            && !locked
        {
            pt.pos = to;
        }
        // Move whole circles when their centre moves (keeps the radius).
        Ok(())
    })?;
    Ok(json!({"sketch": info}))
}

fn solve_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target_sketch(s, p, "sketch.solve")?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("the sketch has not been evaluated".into()))?;
    Ok(json!({"status": ss.report.status, "dof": ss.report.dof, "failing": ss.report.failing, "profiles": ss.profiles.len()}))
}

#[cfg(test)]
#[path = "sketch_rect_tests.rs"]
mod rect_tests;

#[cfg(test)]
#[path = "sketch_ux_tests.rs"]
mod ux_tests;
