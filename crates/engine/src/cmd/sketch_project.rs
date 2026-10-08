//! Sketch projection: Project, Intersect and Include 3D Geometry bring model edges, faces,
//! bodies, other sketches and the origin into the active sketch as linked reference geometry
//! (fixed, drawn purple, updated when the upstream geometry changes). Break Link turns them into
//! normal geometry.

use serde_json::{Value, json};
use solvecraft_doc::project::resolve;
use solvecraft_doc::{Document, FeatureKind};
use solvecraft_geom::{Plane, Vec3};
use solvecraft_sketch::{ConstraintKind, LinkKind, LinkSource, Sketch};

use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, str_, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("ProjectNewCmd", "Project", project_cmd).at("SKETCH", "CREATE").icon("project").key("P").enabled(in_sketch).params(REFS_DOC),
    CommandSpec::new("IntersectCmd", "Intersect", intersect_cmd).at("SKETCH", "CREATE").icon("intersect").enabled(in_sketch).params(
        "refs: [{body}, {face: [x,y,z], body?}, {edge: [x,y,z], body?}, {sketch, curve}]: section curves (or points) with the sketch plane, linked",
    ),
    CommandSpec::new("Include3DGeometry", "Include 3D Geometry", include_cmd)
        .at("SKETCH", "CREATE")
        .icon("include")
        .enabled(in_sketch)
        .params("refs: like Project (edges, vertices, sketch curves and points); curves out of the sketch plane are flattened onto it"),
    CommandSpec::new("ProjectToSurface", "Project To Surface", project_to_surface)
        .at("SKETCH", "CREATE")
        .icon("project_surface")
        .enabled(in_sketch)
        .params("curves: [{sketch, curve}…] (curves of other sketches), face: [x,y,z] point on the target face, body?: projected along the active sketch's normal onto the face, as linked 3D curves"),
    CommandSpec::new("IntersectionCurve", "Intersection Curve", intersection_curve)
        .at("SKETCH", "CREATE")
        .icon("intersection_curve")
        .enabled(in_sketch)
        .params("a, b: {body} | {face: [x,y,z], body?}: where they meet, as linked 3D curves"),
    CommandSpec::new("SpunProfileCmd", "Spun Profile", spun_profile)
        .at("SKETCH", "CREATE")
        .icon("spun")
        .enabled(in_sketch)
        .params("body: name, axis: a line of the active sketch (curve id) | X|Y|Z: the body's outline revolved about the axis, in the sketch plane"),
    CommandSpec::new("SketchIsoparametricCurve", "Isoparametric Curve", iso_curve)
        .at("SKETCH", "CREATE")
        .icon("iso_curve")
        .enabled(in_sketch)
        .params("face: [x,y,z] point on the face (the curve passes through it), body?, direction?: u|v|[x,y,z] (default u): linked 3D curve"),
    CommandSpec::new("FitCurvesToSectionCommand", "Fit Curves to Mesh Section", fit_section)
        .at("SKETCH", "CREATE")
        .icon("fit_section")
        .enabled(in_sketch)
        .params("body: name (a mesh or solid body): its section with the sketch plane, fitted with lines and arcs, as normal (unlinked) geometry"),
    CommandSpec::new("sketch.break_link", "Break Link", break_link)
        .enabled(in_sketch)
        .params("link?: link id (j1) | entities?: [curve or point ids]: linked geometry becomes normal sketch geometry"),
    CommandSpec::new("sketch.auto_project", "Auto-Project Edges on Reference", auto_project)
        .noundo()
        .params("value?: bool (default toggles): project a face's edges when a sketch is created on it"),
];

const REFS_DOC: &str = "refs: [{edge: [x,y,z], body?} | {face: [x,y,z], body?} (its edge loops) | {body: name} (silhouette) | {vertex: [x,y,z], body?} | {sketch, curve} | {sketch, point} | \"origin\" | {axis: X|Y|Z} | {plane: XY|XZ|YZ|name}]";

/// Parse one reference.
fn source(s: &Session, v: &Value, cmd: &str) -> Result<LinkSource> {
    if let Some(name) = v.as_str() {
        return match name.to_ascii_lowercase().as_str() {
            "origin" => Ok(LinkSource::Origin),
            "x" | "y" | "z" => Ok(LinkSource::Axis { name: name.to_ascii_uppercase() }),
            _ if s.model.state().body(name).is_some() => Ok(LinkSource::Body { body: name.to_string() }),
            _ => Err(bad(cmd, format!("unknown reference `{name}`"))),
        };
    }
    let body = || str_(v, "body").unwrap_or("").to_string();
    if let Some(at) = v.get("edge").and_then(vec3) {
        return Ok(LinkSource::Edge { body: body(), at });
    }
    if let Some(at) = v.get("face").and_then(vec3) {
        return Ok(LinkSource::Face { body: body(), at });
    }
    if let Some(at) = v.get("vertex").and_then(vec3) {
        return Ok(LinkSource::Vertex { body: body(), at });
    }
    if let Some(sk) = v.get("sketch") {
        let key = match sk {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err(bad(cmd, "`sketch` must be an id or a name")),
        };
        let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
        if !matches!(f.kind, FeatureKind::Sketch { .. }) {
            return Err(bad(cmd, format!("`{key}` is not a sketch")));
        }
        if Some(f.id) == s.active_sketch {
            return Err(bad(cmd, "a sketch cannot project its own geometry"));
        }
        if let Some(c) = str_(v, "curve") {
            return Ok(LinkSource::SketchCurve { sketch: f.id, curve: c.to_string() });
        }
        if let Some(p) = str_(v, "point") {
            return Ok(LinkSource::SketchPoint { sketch: f.id, point: p.to_string() });
        }
        return Err(bad(cmd, "a sketch reference needs `curve` or `point`"));
    }
    if let Some(b) = str_(v, "body") {
        return Ok(LinkSource::Body { body: b.to_string() });
    }
    if let Some(a) = str_(v, "axis") {
        return Ok(LinkSource::Axis { name: a.to_ascii_uppercase() });
    }
    if let Some(p) = str_(v, "plane") {
        return Ok(LinkSource::Plane { name: p.to_string() });
    }
    if v.get("origin").is_some() {
        return Ok(LinkSource::Origin);
    }
    Err(bad(cmd, format!("cannot project {v}")))
}

/// The plane of a sketch feature.
fn sketch_plane(doc: &Document, id: u64) -> Result<Plane> {
    let (vals, _) = doc.param_values();
    match doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Sketch { plane, .. }) => Ok(doc.resolve_plane(&vals, plane, 0)?),
        _ => Err(EngineError::Other(format!("feature {id} is not a sketch"))),
    }
}

/// Re-resolve the links of a sketch being edited against the model before it, so edits see
/// current reference geometry. Returns warnings.
pub(super) fn refresh(s: &Session, doc: &Document, id: u64, sk: &mut Sketch) -> Vec<String> {
    if sk.links.is_empty() {
        return Vec::new();
    }
    let Ok(plane) = sketch_plane(doc, id) else { return Vec::new() };
    let (vals, _) = doc.param_values();
    let st = s.model.state_before(id);
    solvecraft_doc::project::refresh_links(doc, &vals, &st, &plane, sk)
}

/// Add a link for `src` to sketch `id` in `doc` (not stored in the session). Returns the link id.
pub(super) fn add_link(s: &Session, doc: &Document, id: u64, sk: &mut Sketch, kind: LinkKind, src: LinkSource) -> Result<String> {
    let plane = sketch_plane(doc, id)?;
    let (vals, _) = doc.param_values();
    let st = s.model.state_before(id);
    let r = resolve(doc, &vals, &st, &plane, kind, &src)?;
    Ok(sk.add_link_with_wires(kind, r.source, &r.geom, &r.wires)?)
}

fn link_json(sk: &Sketch, link: &str) -> Value {
    let curves: Vec<String> = sk.link_curves(link).iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
    let points: Vec<String> = sk.link_points(link).iter().filter_map(|p| sk.points.get(*p).map(|p| p.id.clone())).collect();
    json!({"link": link, "curves": curves, "points": points})
}

fn run_links(s: &mut Session, p: &Value, cmd: &str, kind: LinkKind) -> Result<Value> {
    let refs: Vec<Value> = match p.get("refs").or_else(|| p.get("ref")) {
        Some(Value::Array(a)) => a.iter().take(1000).cloned().collect(),
        Some(v) => vec![v.clone()],
        None => return Err(bad(cmd, "`refs` must list what to project")),
    };
    if refs.is_empty() {
        return Err(bad(cmd, "`refs` is empty"));
    }
    let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let sources: Vec<LinkSource> = refs.iter().map(|v| source(s, v, cmd)).collect::<Result<_>>()?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    refresh(s, &doc, id, &mut sk);
    let mut out = Vec::new();
    for src in sources {
        let what = src.describe();
        let l = add_link(s, &doc, id, &mut sk, kind, src).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
        out.push(link_json(&sk, &l));
    }
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(json!({"links": out}))
}

fn project_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    run_links(s, p, "ProjectNewCmd", LinkKind::Project)
}

fn intersect_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    run_links(s, p, "IntersectCmd", LinkKind::Intersect)
}

fn include_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    run_links(s, p, "Include3DGeometry", LinkKind::Include)
}

/// Add one link from `src` to the active sketch and report it.
fn one_link(s: &mut Session, cmd: &str, kind: LinkKind, src: LinkSource) -> Result<Value> {
    let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    refresh(s, &doc, id, &mut sk);
    let what = src.describe();
    let l = add_link(s, &doc, id, &mut sk, kind, src).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
    let mut out = link_json(&sk, &l);
    let wires: Vec<String> = sk.link_wires(&l).iter().filter_map(|w| sk.wires.get(*w).map(|w| w.id.clone())).collect();
    out["wires"] = json!(wires);
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(out)
}

fn project_to_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ProjectToSurface";
    let at = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on the target face"))?;
    let body = str_(p, "body").unwrap_or("").to_string();
    let list = p.get("curves").and_then(Value::as_array).cloned().ok_or_else(|| bad(cmd, "`curves` must list {sketch, curve}"))?;
    if list.is_empty() || list.len() > 500 {
        return Err(bad(cmd, "`curves` must list 1…500 curves"));
    }
    let mut out = Vec::new();
    for v in list {
        let LinkSource::SketchCurve { sketch, curve } = source(s, &v, cmd)? else {
            return Err(bad(cmd, "each entry must be {sketch, curve}"));
        };
        out.push(one_link(s, cmd, LinkKind::Project, LinkSource::OnSurface { sketch, curve, body: body.clone(), at })?);
    }
    Ok(json!({"links": out}))
}

fn intersection_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "IntersectionCurve";
    let side = |k: &str| -> Result<LinkSource> {
        let v = p.get(k).ok_or_else(|| bad(cmd, format!("missing `{k}`")))?;
        match source(s, v, cmd)? {
            x @ (LinkSource::Body { .. } | LinkSource::Face { .. }) => Ok(x),
            _ => Err(bad(cmd, format!("`{k}` must be a body or a face"))),
        }
    };
    let (a, b) = (side("a")?, side("b")?);
    one_link(s, cmd, LinkKind::Include, LinkSource::Intersection { a: Box::new(a), b: Box::new(b) })
}

fn spun_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SpunProfileCmd";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must name a body"))?.to_string();
    let axis = str_(p, "axis").ok_or_else(|| bad(cmd, "`axis` must be a sketch line or X|Y|Z"))?;
    let (origin, dir) = match axis.to_ascii_uppercase().as_str() {
        "X" => (Vec3::ZERO, Vec3::X),
        "Y" => (Vec3::ZERO, Vec3::Y),
        "Z" => (Vec3::ZERO, Vec3::Z),
        _ => {
            let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
            let st = s.model.state();
            let ss = st.sketch(id).ok_or_else(|| EngineError::Other("the sketch is not evaluated".into()))?;
            let ci = ss.sketch.curve_index(axis).ok_or_else(|| bad(cmd, format!("unknown curve `{axis}`")))?;
            let Some(solvecraft_sketch::Shape::Line { a, b }) = ss.sketch.shape(ci) else { return Err(bad(cmd, "the axis must be a line")) };
            let (wa, wb) = (ss.plane.to_world(a), ss.plane.to_world(b));
            (wa, wb - wa)
        }
    };
    one_link(s, cmd, LinkKind::Project, LinkSource::Spun { body, origin, dir })
}

fn iso_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchIsoparametricCurve";
    let at = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on the face"))?;
    let body = str_(p, "body").unwrap_or("").to_string();
    let dir = match p.get("direction") {
        None => "u".to_string(),
        Some(Value::String(d)) if d.eq_ignore_ascii_case("u") || d.eq_ignore_ascii_case("v") => d.to_ascii_lowercase(),
        Some(v) => {
            let d = vec3(v).ok_or_else(|| bad(cmd, "`direction` must be u, v or [x,y,z]"))?;
            format!("{},{},{}", d.x, d.y, d.z)
        }
    };
    one_link(s, cmd, LinkKind::Include, LinkSource::Iso { body, at, dir })
}

fn fit_section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FitCurvesToSectionCommand";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must name a body"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let l = add_link(s, &doc, id, &mut sk, LinkKind::Intersect, LinkSource::Body { body }).map_err(|e| bad(cmd, e.to_string()))?;
    let curves: Vec<String> = sk.link_curves(&l).iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
    sk.break_link(&l)?;
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(json!({"curves": curves}))
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.break_link";
    let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(id)?;
    let mut links: Vec<String> = Vec::new();
    if let Some(l) = str_(p, "link") {
        if sk.link(l).is_none() {
            return Err(bad(cmd, format!("no link `{l}`")));
        }
        links.push(l.to_string());
    }
    for e in crate::params::string_list(p, "entities") {
        let l = sk
            .curves
            .iter()
            .find(|c| c.id == e)
            .and_then(|c| c.link.clone())
            .or_else(|| sk.points.iter().find(|q| q.id == e).and_then(|q| q.link.clone()))
            .ok_or_else(|| bad(cmd, format!("`{e}` is not linked geometry")))?;
        if !links.contains(&l) {
            links.push(l);
        }
    }
    if links.is_empty() {
        return Err(bad(cmd, "give `link` or `entities`"));
    }
    for l in &links {
        sk.break_link(l)?;
    }
    *s.doc_mut() = doc;
    Ok(json!({"broken": links}))
}

fn auto_project(s: &mut Session, p: &Value) -> Result<Value> {
    s.auto_project = bool_(p, "value").unwrap_or(!s.auto_project);
    Ok(json!({"auto_project": s.auto_project}))
}

/// Point arguments that snap to model geometry while drawing, rewritten to sketch point ids:
/// `{"vertex": [x,y,z]}` projects the vertex (a linked point); `{"on_edge": [x,y,z]}` projects
/// the edge and makes a free point on it (coincident with the projection); `"mid:<line>"`
/// makes a point held at the line's midpoint (and `"vertex:x,y,z"` is the vertex form). Everything
/// else is left as it is. The links are added to the active sketch in the session's document.
pub(super) fn snap_points(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let Some(id) = s.active_sketch else { return Ok(p.clone()) };
    let mut out = p.clone();
    let mut doc: Option<(Document, Sketch)> = None;
    let fix = |v: &mut Value, s: &Session, doc: &mut Option<(Document, Sketch)>| -> Result<()> {
        let (vertex, edge) = (v.get("vertex").and_then(vec3), v.get("on_edge").and_then(vec3));
        // String forms from interactive snapping: "vertex:x,y,z" and "mid:<line>".
        let vertex = vertex.or_else(|| {
            let t = v.as_str()?.strip_prefix("vertex:")?;
            let n: Vec<f64> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            match n[..] {
                [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => Some(Vec3::new(x, y, z)),
                _ => None,
            }
        });
        let mid = v.as_str().and_then(|x| x.strip_prefix("mid:")).map(str::to_string);
        // "on:<curve>:x,y": a point on a curve of the sketch near x,y.
        let on = v.as_str().and_then(|x| x.strip_prefix("on:")).and_then(|t| {
            let (c, xy) = t.rsplit_once(':')?;
            let n: Vec<f64> = xy.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            match n[..] {
                [x, y] if x.is_finite() && y.is_finite() => Some((c.to_string(), solvecraft_geom::Vec2::new(x, y))),
                _ => None,
            }
        });
        if vertex.is_none() && edge.is_none() && mid.is_none() && on.is_none() {
            return Ok(());
        }
        if doc.is_none() {
            let d = (*s.doc).clone();
            let mut sk = d.sketch(id)?.clone();
            refresh(s, &d, id, &mut sk);
            *doc = Some((d, sk));
        }
        let Some((d, sk)) = doc.as_mut() else { return Ok(()) };
        let body = str_(v, "body").unwrap_or("").to_string();
        let pid = if let Some((curve, near)) = on {
            let c = sk.curve_index(&curve).ok_or_else(|| bad(cmd, format!("unknown curve `{curve}`")))?;
            let at = match sk.shape(c) {
                Some(sh) => sh.project(near),
                None => near,
            };
            let pi = sk.add_point(at, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(line) = mid {
            // "mid:<line>": a new point at the line's midpoint, kept there.
            let l = sk.curve_index(&line).ok_or_else(|| bad(cmd, format!("unknown curve `{line}`")))?;
            let Some(solvecraft_sketch::Shape::Line { a, b }) = sk.shape(l) else { return Err(bad(cmd, format!("`{line}` is not a line"))) };
            let pi = sk.add_point((a + b) * 0.5, None)?;
            sk.add_constraint(ConstraintKind::Midpoint { p: pi, l }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(at) = vertex {
            let l = add_link(s, d, id, sk, LinkKind::Project, LinkSource::Vertex { body, at }).map_err(|e| bad(cmd, format!("vertex: {e}")))?;
            let pi = sk.link_points(&l).first().copied().ok_or_else(|| bad(cmd, "vertex projects to nothing"))?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(at) = edge {
            let l = add_link(s, d, id, sk, LinkKind::Project, LinkSource::Edge { body, at }).map_err(|e| bad(cmd, format!("edge: {e}")))?;
            let plane = sketch_plane(d, id)?;
            let q = plane.to_local(at);
            let ci = sk.link_curves(&l).first().copied().ok_or_else(|| bad(cmd, "the edge projects to a point"))?;
            let pi = sk.add_point(q, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c: ci }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else {
            return Ok(());
        };
        *v = Value::String(pid);
        Ok(())
    };
    if let Some(o) = out.as_object_mut() {
        for (_, v) in o.iter_mut() {
            match v {
                Value::Array(a) if a.len() <= 10_000 => {
                    for x in a.iter_mut() {
                        fix(x, s, &mut doc)?;
                    }
                }
                Value::Object(_) | Value::String(_) => fix(v, s, &mut doc)?,
                _ => {}
            }
        }
    }
    if let Some((mut d, sk)) = doc {
        *d.sketch_mut(id)? = sk;
        *s.doc_mut() = d;
    }
    Ok(out)
}

/// Project the edges of the face a new sketch was created on (Fusion's default).
pub(super) fn auto_project_face(s: &mut Session, id: u64, at: Vec3) -> Result<Option<String>> {
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let l = match add_link(s, &doc, id, &mut sk, LinkKind::Project, LinkSource::Face { body: String::new(), at }) {
        Ok(l) => l,
        Err(_) => return Ok(None),
    };
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(Some(l))
}

#[cfg(test)]
#[path = "sketch_project_tests.rs"]
mod tests;
