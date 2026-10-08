//! Solid feature commands: extrude, revolve, fillet, chamfer, primitives, combine, move.

use serde_json::{Value, json};
use solvecraft_doc::{AxisRef, Direction, Extent, FeatureKind, Operation, ProfileSel, expr::Kind};
use solvecraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, expr, req_expr, str_, string_list, vec2, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("solid.extrude", "Extrude", extrude)
        .at("SOLID", "CREATE")
        .icon("extrude")
        .key("E")
        .params("distance: expr (or through_all: true); taper?: angle expr; sketch?: id|name (default: active or last sketch); profiles?: all | [index] | [[curve ids]] | [{point:[x,y]}]; face?: [x,y,z] (extrude a planar body face instead of a sketch profile); direction?: positive|negative|symmetric; distance2?; start_offset?; to?: [x,y,z] (To Object: a face, vertex or point; to_offset?: expr past it); from?: [x,y,z] (From Object: start at that face or point); operation?: new|join|cut|intersect|auto (cut into a body, join out of one, else new); targets?: [body]; name?; body_name?"),
    CommandSpec::new("solid.revolve", "Revolve", revolve)
        .at("SOLID", "CREATE")
        .icon("revolve")
        .params("axis: sketch line id | x | y (sketch axes) | X|Y|Z (world); angle?: expr (default 360 deg); angle2?: expr (Two Sides: the other side's angle); direction?: symmetric (angle split either side); to?: [x,y,z] (To Object: turn until that face, vertex or point); sketch?, profiles? | face: [x,y,z] (a planar body face as the profile), operation?, targets?, name?, body_name?"),
    CommandSpec::new("solid.sweep", "Sweep", sweep)
        .at("SOLID", "CREATE")
        .icon("sweep")
        .params("sketch: profile sketch; profiles?; path_sketch: sketch; path: [curve ids in order]; operation?"),
    CommandSpec::new("solid.loft", "Loft", loft).at("SOLID", "CREATE").icon("loft").params("sections: [{sketch, profiles?} | {sketch, point: id} (an apex)] (2 or more, in order); operation?"),
    CommandSpec::new("solid.box", "Box", prim_box).at("SOLID", "CREATE").icon("box").params("length, width, height: expr; corner?: [x,y,z] | center?: [x,y,z]; operation?"),
    CommandSpec::new("solid.cylinder", "Cylinder", prim_cylinder).at("SOLID", "CREATE").icon("cylinder").params("radius | diameter, height: expr; base?: [x,y,z]; axis?: [x,y,z]; operation?"),
    CommandSpec::new("solid.sphere", "Sphere", prim_sphere).at("SOLID", "CREATE").icon("sphere").params("radius | diameter: expr; center?: [x,y,z]; operation?"),
    CommandSpec::new("solid.torus", "Torus", prim_torus).at("SOLID", "CREATE").icon("torus").params("major, minor: expr (radii); center?; operation?"),
    CommandSpec::new("solid.fillet", "Fillet", fillet)
        .at("SOLID", "MODIFY")
        .icon("fillet")
        .key("F")
        .params("edges: [[x,y,z] point on edge | {body, index}]; radius: expr; body?; type?: constant|chord (radius is the width across)|variable (radius at the end nearest start, radius2 at the other); radius2?; start?: [x,y,z]"),
    CommandSpec::new("solid.chamfer", "Chamfer", chamfer).at("SOLID", "MODIFY").icon("chamfer").params("edges: [[x,y,z] | {body, index}]; distance: expr; distance2?: expr (along the second face) | angle?: expr (from the first face); flip?: bool (which face is first: by default the one facing up most); body?"),
    CommandSpec::new("solid.combine", "Combine", combine).at("SOLID", "MODIFY").icon("combine").params("target: body; tools: [body]; operation?: join|cut|intersect; keep_tools?: bool; new_component?: bool (the result goes into a new component; component_name?)"),
    CommandSpec::new("solid.pattern.rectangular", "Rectangular Pattern", pattern_rect)
        .at("SOLID", "CREATE")
        .icon("pattern_rect")
        .params("features: [names] | bodies: [names] | components: [names]; dir1: [x,y,z]; count1; spacing1; dir2?, count2?, spacing2?"),
    CommandSpec::new("solid.pattern.circular", "Circular Pattern", pattern_circ)
        .at("SOLID", "CREATE")
        .icon("pattern_circ")
        .params("features: [names] | bodies: [names] | components: [names]; axis: X|Y|Z | {origin, dir}; count; angle? (default 360 deg)"),
    CommandSpec::new("solid.pattern.path", "Pattern on Path", pattern_path)
        .at("SOLID", "CREATE")
        .icon("pattern_rect")
        .params("features: [names] | bodies: [names]; path_sketch: sketch; path: [curve ids in order]; count; spacing (between instances) | distance (first to last); orientation?: identical|path; flip?: bool"),
    CommandSpec::new("solid.mirror", "Mirror", mirror).at("SOLID", "CREATE").icon("mirror").params("features: [names] | bodies: [names] (combine?: join with the original); plane: XY|XZ|YZ | {origin, x_dir, y_dir}"),
    CommandSpec::new("solid.hole", "Hole", hole)
        .at("SOLID", "CREATE")
        .icon("hole")
        .key("H")
        .params("position: [x,y,z] on a face | sketch + points: [sketch point ids] (one hole each, perpendicular to the sketch) | sketch alone (every sketch point: standalone points and circle centres); direction?: [x,y,z] (default: into the face); diameter | thread (a tapped hole: the tap drill, major − pitch) | clearance: \"M6\" with fit?: close|normal|loose (ISO 273); depth? (default through all) | to?: [x,y,z] (To Object: drill to that face or point); type?: simple|drilled|counterbore|countersink; tip_angle?; cb_diameter?, cb_depth?; cs_diameter?, cs_angle?; thread?: \"M6\" (cosmetic)"),
    CommandSpec::new("solid.pipe", "Pipe", pipe)
        .at("SOLID", "CREATE")
        .icon("pipe")
        .params("path_sketch: sketch; path: [curve ids in order]; diameter: expr; wall?: expr (hollow); operation?, targets?, name?, body_name?"),
    CommandSpec::new("solid.bounding_solid", "Bounding Solid", bounding_solid)
        .at("SOLID", "CREATE")
        .icon("box")
        .params("bodies?: [names] (default: all); margin?: expr; name?, body_name?"),
    CommandSpec::new("solid.scale", "Scale", scale)
        .at("SOLID", "MODIFY")
        .icon("scale")
        .params("bodies: [names]; factor: expr | factors: [x, y, z] exprs; origin?: [x,y,z]"),
    CommandSpec::new("face.delete", "Delete Face", delete_face)
        .at("SOLID", "MODIFY")
        .icon("delete")
        .params("faces: [[x,y,z] points on the faces]; body?. The neighbouring planes close the gap, as when a rounded edge, hole or boss is taken off"),
    CommandSpec::new("solid.offset_face", "Offset Face", offset_face)
        .at("SOLID", "MODIFY")
        .icon("offset_face")
        .params("faces: [[x,y,z] points on planar faces]; distance: expr (positive: outward); body?"),
    CommandSpec::new("solid.thread", "Thread", thread)
        .at("SOLID", "CREATE")
        .icon("thread")
        .params("face: [x,y,z] on a cylindrical face; designation?: ISO metric (\"M8\", \"M8x1\"; default: the size that fits); length?: expr (default: the whole face). Cosmetic: the model is unchanged, model.threads lists it"),
    CommandSpec::new("solid.shell", "Shell", shell)
        .at("SOLID", "MODIFY")
        .icon("shell")
        .params("faces: [[x,y,z] points on the faces to remove]; thickness: expr; direction?: inside|outside|both; tangent_chain?: bool (also open faces joined smoothly to the picked ones); body?"),
    CommandSpec::new("solid.draft", "Draft", draft)
        .at("SOLID", "MODIFY")
        .icon("draft")
        .params("faces: [[x,y,z]]; angle: expr; neutral: XY|XZ|YZ|plane name|{origin, normal}; pull?: [x,y,z] (default: neutral plane normal); body?"),
    CommandSpec::new("construct.plane.offset", "Offset Plane", plane_offset)
        .at("SOLID", "CONSTRUCT")
        .icon("plane")
        .params("base: XY|XZ|YZ|plane name; offset: expr; name?"),
    CommandSpec::new("construct.plane.at_angle", "Plane at Angle", plane_angle)
        .at("SOLID", "CONSTRUCT")
        .icon("plane")
        .params("base: XY|XZ|YZ|plane name; axis: X|Y|Z|{origin, dir}; angle: expr; name?"),
    CommandSpec::new("solid.split_face", "Split Face", split_face)
        .at("SOLID", "MODIFY")
        .icon("split")
        .params(
            "faces: [[x,y,z] points on the faces to split]; body?; and the splitting tool: plane: XY|XZ|YZ|plane name|{origin, normal} — or tool: {body, point} (a face of a body, its surface extended) — or sketch: id or name (its curves, along the sketch's normal)",
        ),
    CommandSpec::new("solid.split_body", "Split Body", split_body)
        .at("SOLID", "MODIFY")
        .icon("split")
        .params("body: name; plane: XY|XZ|YZ|plane name|{origin, normal} — or tool: {body, point} (a face of a body, its surface extended)"),
    CommandSpec::new("solid.press_pull", "Press Pull", press_pull)
        .at("SOLID", "MODIFY")
        .icon("presspull")
        .key("Q")
        .params("face: [x,y,z] and distance: expr (positive adds material, negative cuts), or edges: [[x,y,z]…] and distance (a fillet)"),
    CommandSpec::new("solid.move", "Move/Copy", move_bodies).at("SOLID", "MODIFY").icon("move").key("M").params("bodies: [names]; translate?: [x,y,z] (exprs or numbers); axis?: [x,y,z]; angle?: expr"),
];

/// Where an extrude starts and which way is positive: a point inside the (first) profile or
/// face, and the unit normal.
fn extrude_base(s: &Session, p: &Value) -> Option<(Vec3, Vec3)> {
    if let Some(fp) = p.get("face").and_then(vec3) {
        return Some((fp, super::face::face_normal(s, fp)?));
    }
    let sid = feature_sketch(s, p, "Extrude").ok()?;
    let st = s.model.state();
    let ss = st.sketch(sid)?;
    let ps = &ss.profiles;
    let q = match profiles(p, "Extrude").ok()? {
        ProfileSel::All => ps.first()?.region.interior_point(),
        ProfileSel::Indices { indices } => ps.get(*indices.first()?)?.region.interior_point(),
        ProfileSel::Points { points } => *points.first()?,
        ProfileSel::Curves { loops } => {
            let mut want: Vec<&String> = loops.first()?.iter().collect();
            want.sort();
            ps.iter()
                .find(|x| {
                    let mut have: Vec<&String> = x.outer_curves.iter().collect();
                    have.sort();
                    have == want
                })?
                .region
                .interior_point()
        }
    };
    Some((ss.plane.to_world(q), ss.plane.normal()))
}

/// What `operation: "auto"` means for an extrude with these parameters: `cut` when it goes
/// into a body, `join` when it grows out of one, `new` otherwise.
pub fn auto_operation(s: &Session, p: &Value) -> Option<&'static str> {
    let d = s.doc.eval(&expr(p, "distance")?, Kind::Length).ok()?;
    let dir = str_(p, "direction").map(str::to_ascii_lowercase);
    let symmetric = dir.as_deref() == Some("symmetric");
    let d = if matches!(dir.as_deref(), Some("negative" | "reverse" | "flip")) { -d } else { d };
    let (base, n) = extrude_base(s, p)?;
    let st = s.model.state();
    if st.bodies.is_empty() || !d.is_finite() {
        return Some("new");
    }
    // Only the active component's bodies take part (or the ones named as participants).
    let parts = string_list(p, "participants");
    let inside = |q: Vec3| {
        st.bodies
            .iter()
            .filter(|b| if parts.is_empty() { s.doc.body_component(&b.name, b.feature) == s.active_component } else { parts.contains(&b.name) })
            .any(|b| b.mesh().contains(q))
    };
    let eps = (d.abs() * 1e-3).max(1e-3);
    let sign = if d < 0.0 { -1.0 } else { 1.0 };
    if symmetric {
        return Some(if inside(base + n * (d.abs() * 0.5)) || inside(base - n * (d.abs() * 0.5)) { "cut" } else { "new" });
    }
    if inside(base + n * (d * 0.5)) {
        Some("cut")
    } else if inside(base - n * (eps * sign)) {
        Some("join")
    } else {
        Some("new")
    }
}

pub(super) fn operation(p: &Value, cmd: &str) -> Result<Operation> {
    match str_(p, "operation") {
        None => Ok(Operation::NewBody),
        Some(o) => Operation::parse(o).ok_or_else(|| bad(cmd, format!("unknown operation `{o}` (new, join, cut, intersect)"))),
    }
}

/// The sketch a feature uses: `sketch` param, the active sketch, or the last sketch.
pub(super) fn feature_sketch(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    if let Some(v) = p.get("sketch") {
        let key = match v {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err(bad(cmd, "`sketch` must be an id or name")),
        };
        let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
        return match f.kind {
            FeatureKind::Sketch { .. } => Ok(f.id),
            _ => Err(bad(cmd, format!("`{key}` is not a sketch"))),
        };
    }
    if let Some(a) = s.active_sketch {
        return Ok(a);
    }
    let limit = s.doc.marker.unwrap_or(usize::MAX);
    s.doc
        .features
        .iter()
        .take(limit)
        .rev()
        .find(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
        .map(|f| f.id)
        .ok_or_else(|| bad(cmd, "there is no sketch to use (create one first)"))
}

pub(super) fn profiles(p: &Value, cmd: &str) -> Result<ProfileSel> {
    let Some(v) = p.get("profiles") else { return Ok(ProfileSel::All) };
    match v {
        Value::String(s) if s.eq_ignore_ascii_case("all") => Ok(ProfileSel::All),
        Value::Number(n) => Ok(ProfileSel::Indices { indices: vec![n.as_u64().ok_or_else(|| bad(cmd, "profile index"))? as usize] }),
        Value::Array(a) if a.is_empty() => Err(bad(cmd, "`profiles` is empty")),
        Value::Array(a) if a.len() > 10_000 => Err(bad(cmd, "too many profiles")),
        Value::Array(a) => {
            if a.iter().all(Value::is_u64) {
                return Ok(ProfileSel::Indices { indices: a.iter().filter_map(Value::as_u64).map(|x| x as usize).collect() });
            }
            if a.iter().all(|x| x.get("point").is_some()) {
                let pts =
                    a.iter().map(|x| x.get("point").and_then(vec2).ok_or_else(|| bad(cmd, "profile point must be [x, y]"))).collect::<Result<_>>()?;
                return Ok(ProfileSel::Points { points: pts });
            }
            // [[ids]], [{curves: [...]}] or recipe-style [{loops: [{outer: true, curves: [...]}]}].
            let mut loops = Vec::new();
            for x in a {
                let ids: Vec<String> = match x {
                    Value::Array(ids) => ids.iter().filter_map(|i| i.as_str().map(str::to_string)).collect(),
                    Value::Object(o) => {
                        if let Some(c) = o.get("curves") {
                            string_list(&json!({"c": c}), "c")
                        } else if let Some(ls) = o.get("loops").and_then(Value::as_array) {
                            let outer = ls.iter().find(|l| l.get("outer").and_then(Value::as_bool).unwrap_or(true)).or(ls.first());
                            outer.map(|l| string_list(l, "curves")).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                };
                if ids.is_empty() {
                    return Err(bad(cmd, "each profile must be an index, a list of curve ids, {curves}, {loops} or {point}"));
                }
                loops.push(ids);
            }
            Ok(ProfileSel::Curves { loops })
        }
        _ => Err(bad(cmd, "`profiles` must be \"all\" or a list")),
    }
}

pub(super) fn check_expr(s: &Session, e: &str, kind: Kind, cmd: &str, what: &str) -> Result<()> {
    s.doc.eval(e, kind).map(|_| ()).map_err(|err| bad(cmd, format!("{what}: {err}")))
}

/// Add a feature, evaluate, and report its result (an evaluation error fails the command).
pub(super) fn add_feature(s: &mut Session, p: &Value, kind: FeatureKind) -> Result<Value> {
    let name = str_(p, "name");
    // Commands speak world coordinates; the feature is authored in the active component.
    let mut kind = kind;
    super::component::to_active_frame(s, &mut kind);
    // Bare lengths typed in an inch (or cm…) design mean that unit.
    if s.doc.units.trim() != "mm" {
        let doc = s.doc.clone();
        for (_, e, k) in kind.inputs_mut() {
            *e = doc.with_design_unit(e, k);
        }
    }
    // Blends remember where their edges sat, to find them again after upstream edits.
    let (refs, names) = match &kind {
        FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } => {
            let st = s.model.state();
            (solvecraft_doc::edge_refs(&st, body, edges), solvecraft_doc::edge_names_for(&st, body, edges))
        }
        // Sheet metal edges are named by the sheet model.
        FeatureKind::SheetFlange { edges, .. } | FeatureKind::SheetHem { edges, .. } => {
            (Vec::new(), solvecraft_doc::sheet::sheet_edge_names(&s.model.state(), edges))
        }
        _ => (Vec::new(), Vec::new()),
    };
    let face_names = {
        let pts = kind.face_points();
        if pts.is_empty() { Vec::new() } else { solvecraft_doc::naming::face_names_at(&s.model.state(), &pts) }
    };
    let participants = string_list(p, "participants");
    if participants.len() > 10_000 || participants.iter().any(|b| b != "*" && s.model.state().body(b).is_none()) {
        return Err(EngineError::Other("`participants` must list existing bodies".into()));
    }
    let id = s.doc_mut().add_feature(kind, name)?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.participants = participants;
        f.edge_refs = refs;
        f.edge_names = names;
        f.face_names = face_names;
    }
    s.doc_mut().name_feature_inputs(id);
    let comp = s.active_component;
    if comp != 0
        && let Some(f) = s.doc_mut().feature_mut(id)
    {
        f.component = comp;
    }
    let mut names = string_list(p, "body_names");
    if let Some(b) = str_(p, "body_name") {
        names.insert(0, b.to_string());
    }
    if !names.is_empty()
        && let Some(f) = s.doc_mut().feature_mut(id)
    {
        f.body_names = names;
    }
    s.active_sketch = None;
    s.refresh();
    let r = s.model.result(id).cloned();
    if let Some(e) = r.as_ref().and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().map(|b| b.name.as_str()).collect();
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({"feature": id, "name": fname, "bodies": bodies, "ms": r.map(|r| r.ms)}))
}

fn extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.extrude";
    // A planar body face extrudes like a profile: its boundary is copied into a sketch on it.
    let (sketch, face_profile) = match p.get("face").and_then(vec3) {
        Some(fp) => {
            let (id, inside) = super::face::sketch_of_face(s, fp, cmd)?;
            (id, Some(ProfileSel::Points { points: vec![inside] }))
        }
        None => (feature_sketch(s, p, cmd)?, None),
    };
    let through_all = bool_(p, "through_all").unwrap_or(false) || str_(p, "extent").is_some_and(|e| e.eq_ignore_ascii_case("through_all"));
    let to_object = p.get("to").is_some();
    if to_object && p.get("to").and_then(vec3).is_none() {
        return Err(bad(cmd, "`to` must be a point [x, y, z] on the face, vertex or point to extrude to"));
    }
    if p.get("from").is_some() && p.get("from").and_then(vec3).is_none() {
        return Err(bad(cmd, "`from` must be a point [x, y, z] on the face or point to start from"));
    }
    let distance = if through_all || to_object { expr(p, "distance").unwrap_or_else(|| "0".into()) } else { req_expr(cmd, p, "distance")? };
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let taper = expr(p, "taper");
    if let Some(t) = &taper {
        check_expr(s, t, Kind::Angle, cmd, "taper")?;
    }
    let direction = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
        None | Some("positive") | Some("one_side") => Direction::Positive,
        Some("negative") | Some("reverse") | Some("flip") => Direction::Negative,
        Some("symmetric") => Direction::Symmetric,
        Some(o) => return Err(bad(cmd, format!("unknown direction `{o}`"))),
    };
    let distance2 = expr(p, "distance2");
    if let Some(d) = &distance2 {
        check_expr(s, d, Kind::Length, cmd, "distance2")?;
    }
    let start_offset = expr(p, "start_offset");
    if let Some(d) = &start_offset {
        check_expr(s, d, Kind::Length, cmd, "start_offset")?;
    }
    let kind = FeatureKind::Extrude {
        sketch,
        profiles: match face_profile {
            Some(f) => f,
            None => profiles(p, cmd)?,
        },
        extent: Extent {
            distance,
            direction,
            distance2,
            start_offset,
            through_all,
            taper,
            to: p.get("to").and_then(vec3),
            to_offset: expr(p, "to_offset"),
            from: p.get("from").and_then(vec3),
        },
        operation: match str_(p, "operation") {
            Some(o) if o.eq_ignore_ascii_case("auto") => Operation::parse(auto_operation(s, p).unwrap_or("new")).unwrap_or(Operation::NewBody),
            _ => operation(p, cmd)?,
        },
        targets: string_list(p, "targets"),
    };
    add_feature(s, p, kind)
}

fn revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.revolve";
    // A planar body face revolves like a profile (its edges projected into a sketch on it).
    let (sketch, face_profile) = match p.get("face").and_then(vec3) {
        Some(fp) => {
            let (id, inside) = super::face::sketch_of_face(s, fp, cmd)?;
            s.active_sketch = None;
            (id, Some(ProfileSel::Points { points: vec![inside] }))
        }
        None => (feature_sketch(s, p, cmd)?, None),
    };
    let axis = if let Some(o) = p.get("axis").filter(|v| v.is_object()) {
        let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
        let dir = o.get("dir").and_then(vec3).ok_or_else(|| bad(cmd, "axis needs `dir`"))?;
        AxisRef::Line { origin, dir }
    } else {
        match str_(p, "axis") {
            Some(a @ ("X" | "Y" | "Z")) => AxisRef::World { axis: a.into() },
            Some(a @ ("x" | "y")) => AxisRef::SketchAxis { axis: a.into() },
            Some(id) => AxisRef::SketchLine { curve: id.into() },
            None => return Err(bad(cmd, "`axis` must be a sketch line id, x, y (sketch axes), X, Y, Z or {origin, dir}")),
        }
    };
    let angle = expr(p, "angle").unwrap_or_else(|| "360 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let profiles = match face_profile {
        Some(f) => f,
        None => profiles(p, cmd)?,
    };
    let angle2 = expr(p, "angle2");
    if let Some(a) = &angle2 {
        check_expr(s, a, Kind::Angle, cmd, "angle2")?;
    }
    let to = match p.get("to") {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`to` must be a point [x, y, z] to turn to"))?),
        None => None,
    };
    let symmetric =
        matches!(str_(p, "direction").map(str::to_ascii_lowercase).as_deref(), Some("symmetric")) || bool_(p, "symmetric").unwrap_or(false);
    let kind = FeatureKind::Revolve {
        sketch,
        profiles,
        axis,
        angle,
        operation: operation(p, cmd)?,
        targets: string_list(p, "targets"),
        angle2,
        symmetric,
        to,
    };
    add_feature(s, p, kind)
}

fn radius_expr(p: &Value, cmd: &str) -> Result<String> {
    match (expr(p, "radius"), expr(p, "diameter")) {
        (Some(r), _) => Ok(r),
        (None, Some(d)) => Ok(format!("({d}) / 2")),
        _ => Err(bad(cmd, "needs `radius` or `diameter`")),
    }
}

fn prim_box(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.box";
    let (l, w, h) = (req_expr(cmd, p, "length")?, req_expr(cmd, p, "width")?, req_expr(cmd, p, "height")?);
    for (e, n) in [(&l, "length"), (&w, "width"), (&h, "height")] {
        check_expr(s, e, Kind::Length, cmd, n)?;
    }
    let corner = match (p.get("corner").and_then(vec3), p.get("center").and_then(vec3)) {
        (Some(c), _) => c,
        (None, Some(c)) => {
            let v = |e: &str| s.doc.eval(e, Kind::Length).unwrap_or(0.0);
            c - Vec3::new(v(&l), v(&w), v(&h)) * 0.5
        }
        _ => Vec3::ZERO,
    };
    add_feature(s, p, FeatureKind::Box { corner, length: l, width: w, height: h, operation: operation(p, cmd)? })
}

fn prim_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.cylinder";
    let radius = radius_expr(p, cmd)?;
    let height = req_expr(cmd, p, "height")?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    check_expr(s, &height, Kind::Length, cmd, "height")?;
    let base = p.get("base").and_then(vec3).unwrap_or(Vec3::ZERO);
    let axis = p.get("axis").and_then(vec3).unwrap_or(Vec3::Z);
    add_feature(s, p, FeatureKind::Cylinder { base, axis, radius, height, operation: operation(p, cmd)? })
}

fn prim_sphere(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.sphere";
    let radius = radius_expr(p, cmd)?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    let center = p.get("center").and_then(vec3).unwrap_or(Vec3::ZERO);
    add_feature(s, p, FeatureKind::Sphere { center, radius, operation: operation(p, cmd)? })
}

fn prim_torus(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.torus";
    let (major, minor) = (req_expr(cmd, p, "major")?, req_expr(cmd, p, "minor")?);
    check_expr(s, &major, Kind::Length, cmd, "major")?;
    check_expr(s, &minor, Kind::Length, cmd, "minor")?;
    let center = p.get("center").and_then(vec3).unwrap_or(Vec3::ZERO);
    add_feature(s, p, FeatureKind::Torus { center, major, minor, operation: operation(p, cmd)? })
}

/// Edge references → points on the edges.
fn edge_points(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get("edges").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`edges` must be a list"))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, "select 1…1000 edges"));
    }
    let st = s.model.state();
    let mut out = Vec::new();
    for e in a {
        if let Some(pt) = vec3(e).or_else(|| e.get("point").and_then(vec3)) {
            out.push(pt);
            continue;
        }
        let body = e.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "an edge is a point [x,y,z], {point} or {body, index}"))?;
        let idx = e.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "edge `index` must be a number"))? as usize;
        let b = st.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        let tol = (b.body.size() * 2e-3).max(1e-3);
        let edges = b.body.edges(tol)?;
        out.push(edges.get(idx).ok_or_else(|| bad(cmd, format!("{body} has no edge {idx}")))?.mid);
    }
    Ok(out)
}

/// Press Pull: a face moves along its normal (an extrude that joins outward or cuts inward); an
/// edge is rounded (a fillet).
fn press_pull(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.press_pull";
    let distance = req_expr(cmd, p, "distance")?;
    if p.get("edges").is_some() {
        return fillet(s, &json!({"edges": p.get("edges"), "radius": distance}));
    }
    let face = p.get("face").ok_or_else(|| bad(cmd, "needs `face` or `edges`"))?;
    let d = s.doc.eval(&distance, Kind::Length).map_err(|e| bad(cmd, format!("distance: {e}")))?;
    if d.abs() < 1e-9 {
        return Err(bad(cmd, "the distance must not be zero"));
    }
    // A fillet's or chamfer's face: its size changes by the distance (the feature is edited).
    if let Some(fp) = vec3(face)
        && super::face::face_is_planar(s, fp) == Some(false)
    {
        let name = solvecraft_doc::naming::face_names_at(&s.model.state(), &[fp]).into_iter().next().unwrap_or_default();
        let fid = name.strip_prefix('F').and_then(|r| r.split(':').next()).and_then(|x| x.parse::<u64>().ok());
        if name.contains(":blend") {
            let f = fid.and_then(|id| s.doc.feature(id)).cloned().ok_or_else(|| bad(cmd, "the blend's feature is gone"))?;
            let (key, old) = match &f.kind {
                FeatureKind::Fillet { radius, .. } => ("radius", radius.clone()),
                FeatureKind::Chamfer { distance, .. } => ("distance", distance.clone()),
                _ => return Err(bad(cmd, "that face is not a fillet's or a chamfer's")),
            };
            let new = format!("({old}) + ({distance})");
            let r = super::find_command("timeline.edit").ok_or_else(|| bad(cmd, "timeline.edit"))?;
            let out = (r.run)(s, &json!({"feature": f.id, "set": {key: new}}))?;
            return Ok(json!({"feature": f.id, "edited": f.name, key: new, "recomputed": out.get("recomputed")}));
        }
        // Another curved face moves along its normal.
        let r = super::find_command("solid.offset_face").ok_or_else(|| bad(cmd, "solid.offset_face"))?;
        return (r.run)(s, &json!({"faces": [face], "distance": distance}));
    }
    let (dist, dir, op) = if d > 0.0 { (distance, "positive", "join") } else { (format!("-({distance})"), "negative", "cut") };
    extrude(s, &json!({"face": face, "distance": dist, "direction": dir, "operation": op}))
}

fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.fillet";
    let edges = edge_points(s, p, cmd)?;
    let radius = req_expr(cmd, p, "radius")?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    let style = match str_(p, "type").map(str::to_ascii_lowercase).as_deref() {
        None | Some("constant") | Some("rolling_ball") => solvecraft_doc::FilletStyle::Constant,
        Some("chord") | Some("chord_length") => solvecraft_doc::FilletStyle::Chord,
        Some("variable") => {
            let radius2 = req_expr(cmd, p, "radius2")?;
            check_expr(s, &radius2, Kind::Length, cmd, "radius2")?;
            let start =
                p.get("start").and_then(vec3).ok_or_else(|| bad(cmd, "a variable fillet needs `start`: [x,y,z] near the end that takes `radius`"))?;
            solvecraft_doc::FilletStyle::Variable { radius2, start }
        }
        Some(o) => return Err(bad(cmd, format!("unknown fillet type `{o}` (constant, chord or variable)"))),
    };
    add_feature(s, p, FeatureKind::Fillet { edges, radius, body: str_(p, "body").map(str::to_string), style })
}

fn chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.chamfer";
    let edges = edge_points(s, p, cmd)?;
    let distance = req_expr(cmd, p, "distance")?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let (distance2, angle) = (expr(p, "distance2"), expr(p, "angle"));
    if let Some(d) = &distance2 {
        check_expr(s, d, Kind::Length, cmd, "distance2")?;
    }
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    if distance2.is_some() && angle.is_some() {
        return Err(bad(cmd, "give `distance2` or `angle`, not both"));
    }
    let flip = bool_(p, "flip").unwrap_or(false);
    add_feature(s, p, FeatureKind::Chamfer { edges, distance, distance2, angle, flip, body: str_(p, "body").map(str::to_string) })
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.combine";
    let target = str_(p, "target").ok_or_else(|| bad(cmd, "`target` must be a body name"))?.to_string();
    let tools = string_list(p, "tools");
    if tools.is_empty() {
        return Err(bad(cmd, "`tools` must list bodies"));
    }
    let op = match str_(p, "operation") {
        None => Operation::Join,
        Some(o) => Operation::parse(o).filter(|o| *o != Operation::NewBody).ok_or_else(|| bad(cmd, "operation must be join, cut or intersect"))?,
    };
    let mut out = add_feature(
        s,
        p,
        FeatureKind::Combine { target: target.clone(), tools, operation: op, keep_tools: bool_(p, "keep_tools").unwrap_or(false) },
    )?;
    // New Component: the result goes into a component of its own.
    if bool_(p, "new_component").unwrap_or(false) {
        let name = str_(p, "component_name").map(str::to_string).unwrap_or_else(|| format!("{target} Combined"));
        let c = s.execute("component.create", &json!({"name": name, "activate": false}))?;
        s.execute("component.move_bodies", &json!({"bodies": [target], "component": c["component"]}))?;
        out["component"] = c["component"].clone();
    }
    Ok(out)
}

fn move_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.move";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let t = match p.get("translate") {
        Some(Value::Array(a)) if a.len() == 3 => {
            let mut out: [String; 3] = Default::default();
            for (i, v) in a.iter().enumerate() {
                let e = expr(&json!({"v": v}), "v").ok_or_else(|| bad(cmd, "translate entries must be numbers or expressions"))?;
                check_expr(s, &e, Kind::Length, cmd, "translate")?;
                if let Some(slot) = out.get_mut(i) {
                    *slot = e;
                }
            }
            out
        }
        None => ["0".into(), "0".into(), "0".into()],
        _ => return Err(bad(cmd, "`translate` must be [x, y, z]")),
    };
    let angle = expr(p, "angle");
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    add_feature(s, p, FeatureKind::Move { bodies, translate: t, rotate_axis: p.get("axis").and_then(vec3), angle })
}

fn source_features(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let names = string_list(p, "features");
    if names.is_empty() {
        return Err(bad(cmd, "`features` must list features to copy"));
    }
    for n in &names {
        if s.doc.find_feature(n).is_none() {
            return Err(bad(cmd, format!("no feature `{n}`")));
        }
    }
    Ok(names)
}

fn axis_line(p: &Value, cmd: &str) -> Result<(Vec3, Vec3)> {
    match p.get("axis") {
        Some(Value::String(a)) => match a.to_ascii_uppercase().as_str() {
            "X" => Ok((Vec3::ZERO, Vec3::X)),
            "Y" => Ok((Vec3::ZERO, Vec3::Y)),
            "Z" => Ok((Vec3::ZERO, Vec3::Z)),
            _ => Err(bad(cmd, "axis must be X, Y, Z or {origin, dir}")),
        },
        Some(o @ Value::Object(_)) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let dir = o.get("dir").and_then(vec3).and_then(|d| d.normalized()).ok_or_else(|| bad(cmd, "axis needs a non-zero `dir`"))?;
            Ok((origin, dir))
        }
        _ => Err(bad(cmd, "`axis` is required")),
    }
}

/// A pattern feature, or for `components`, new occurrences of them at each placement.
fn finish_pattern(s: &mut Session, p: &Value, features: Vec<String>, pattern: solvecraft_doc::PatternKind, bodies: Vec<String>) -> Result<Value> {
    let comps = string_list(p, "components");
    if comps.is_empty() {
        return add_feature(s, p, FeatureKind::Pattern { features, pattern, bodies });
    }
    let cmd = "pattern (components)";
    let mats = solvecraft_doc::pattern_matrices(&s.doc, &s.model.state(), &pattern)?;
    let mut made = Vec::new();
    for c in &comps {
        let id = s.doc.find_component(c).filter(|id| *id != 0).ok_or_else(|| bad(cmd, format!("no component `{c}`")))?;
        let src = s.doc.occurrence_of(id).cloned().ok_or_else(|| bad(cmd, format!("component `{c}` has no occurrence")))?;
        for m in &mats {
            made.push(s.doc_mut().add_occurrence(id, src.parent, solvecraft_doc::mat_mul(m, &src.transform))?);
        }
    }
    Ok(json!({"occurrences": made}))
}

/// What a pattern copies: features (replayed), bodies (`bodies: [names]`), or components
/// (`components: [names]`, placed again as occurrences).
fn pattern_sources(s: &Session, p: &Value, cmd: &str) -> Result<(Vec<String>, Vec<String>)> {
    if !string_list(p, "components").is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Ok((source_features(s, p, cmd)?, Vec::new()));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    Ok((Vec::new(), bodies))
}

fn pattern_rect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.pattern.rectangular";
    let (features, bodies) = pattern_sources(s, p, cmd)?;
    let dir1 = p.get("dir1").and_then(vec3).ok_or_else(|| bad(cmd, "`dir1` must be [x, y, z]"))?;
    let count1 = req_expr(cmd, p, "count1")?;
    let spacing1 = req_expr(cmd, p, "spacing1")?;
    check_expr(s, &count1, Kind::Unitless, cmd, "count1")?;
    check_expr(s, &spacing1, Kind::Length, cmd, "spacing1")?;
    let dir2 = p.get("dir2").and_then(vec3);
    let (count2, spacing2) = (expr(p, "count2"), expr(p, "spacing2"));
    if let Some(c) = &count2 {
        check_expr(s, c, Kind::Unitless, cmd, "count2")?;
    }
    if let Some(c) = &spacing2 {
        check_expr(s, c, Kind::Length, cmd, "spacing2")?;
    }
    let pattern = solvecraft_doc::PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2 };
    finish_pattern(s, p, features, pattern, bodies)
}

fn pattern_circ(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.pattern.circular";
    let (features, bodies) = pattern_sources(s, p, cmd)?;
    let (origin, axis) = axis_line(p, cmd)?;
    let count = req_expr(cmd, p, "count")?;
    check_expr(s, &count, Kind::Unitless, cmd, "count")?;
    let angle = expr(p, "angle").unwrap_or_else(|| "360 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let pattern = solvecraft_doc::PatternKind::Circular { origin, axis, count, angle };
    finish_pattern(s, p, features, pattern, bodies)
}

fn pattern_path(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.pattern.path";
    let (features, bodies) = pattern_sources(s, p, cmd)?;
    let path_sketch = sketch_id(s, p.get("path_sketch").or_else(|| p.get("sketch")), cmd, "path_sketch")?;
    let path = string_list(p, "path");
    if path.is_empty() || path.len() > 1000 {
        return Err(bad(cmd, "`path` must list 1…1000 curve ids"));
    }
    let count = req_expr(cmd, p, "count")?;
    check_expr(s, &count, Kind::Unitless, cmd, "count")?;
    let (spacing, extent) = match (expr(p, "spacing"), expr(p, "distance")) {
        (Some(x), _) => (x, false),
        (None, Some(x)) => (x, true),
        (None, None) => return Err(bad(cmd, "give `spacing` (between instances) or `distance` (first to last)")),
    };
    check_expr(s, &spacing, Kind::Length, cmd, "spacing")?;
    let orient = match str_(p, "orientation") {
        None | Some("identical") => false,
        Some("path" | "path_direction" | "along_path") => true,
        Some(o) => return Err(bad(cmd, format!("unknown orientation `{o}` (identical or path)"))),
    };
    let flip = bool_(p, "flip").unwrap_or(false);
    let pattern = solvecraft_doc::PatternKind::Path { path_sketch, path, count, spacing, extent, flip, orient };
    finish_pattern(s, p, features, pattern, bodies)
}

fn mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.mirror";
    let bodies = string_list(p, "bodies");
    let features = if bodies.is_empty() { source_features(s, p, cmd)? } else { Vec::new() };
    let plane = match p.get("plane") {
        Some(Value::String(n)) if solvecraft_geom::Plane::named(n).is_some() => solvecraft_doc::PlaneRef::Origin { name: n.to_ascii_uppercase() },
        Some(o @ Value::Object(_)) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let pl = match (o.get("x_dir").and_then(vec3), o.get("y_dir").and_then(vec3), o.get("normal").and_then(vec3)) {
                (Some(x), Some(y), _) => solvecraft_geom::Plane::new(origin, x, y),
                (_, _, Some(n)) => solvecraft_geom::Plane::from_normal(origin, n),
                _ => None,
            };
            solvecraft_doc::PlaneRef::Custom { plane: pl.ok_or_else(|| bad(cmd, "plane needs x_dir and y_dir, or normal"))? }
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ or {origin, normal}")),
    };
    let combine = bool_(p, "combine").unwrap_or(false);
    add_feature(s, p, FeatureKind::Mirror { features, plane, bodies, combine })
}

/// A plane reference from a parameter: an origin plane, a construction plane name, or an
/// explicit plane `{origin, x_dir, y_dir}` / `{origin, normal}`.
pub(super) fn plane_param(s: &Session, v: Option<&Value>, cmd: &str) -> Result<solvecraft_doc::PlaneRef> {
    use solvecraft_doc::PlaneRef;
    match v {
        Some(Value::String(n)) if solvecraft_geom::Plane::named(n).is_some() => Ok(PlaneRef::Origin { name: n.to_ascii_uppercase() }),
        Some(Value::String(n)) => match s.doc.find_feature(n).map(|f| &f.kind) {
            Some(FeatureKind::ConstructionPlane { .. }) => Ok(PlaneRef::Construction { name: n.clone() }),
            _ => Err(bad(cmd, format!("no plane `{n}`"))),
        },
        Some(o @ Value::Object(_)) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let pl = match (o.get("x_dir").and_then(vec3), o.get("y_dir").and_then(vec3), o.get("normal").and_then(vec3)) {
                (Some(x), Some(y), _) => solvecraft_geom::Plane::new(origin, x, y),
                (_, _, Some(n)) => solvecraft_geom::Plane::from_normal(origin, n),
                _ => None,
            };
            Ok(PlaneRef::Custom { plane: pl.ok_or_else(|| bad(cmd, "plane needs x_dir and y_dir, or normal"))? })
        }
        _ => Err(bad(cmd, "`plane` must be XY, XZ, YZ, a construction plane name or {origin, normal}")),
    }
}

fn plane_offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.offset";
    let base = plane_param(s, p.get("base"), cmd)?;
    let d = req_expr(cmd, p, "offset")?;
    check_expr(s, &d, Kind::Length, cmd, "offset")?;
    add_feature(s, p, FeatureKind::ConstructionPlane { plane: solvecraft_doc::PlaneRef::Offset { base: Box::new(base), distance: d } })
}

fn plane_angle(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.at_angle";
    let base = plane_param(s, p.get("base"), cmd)?;
    let (axis_origin, axis_dir) = axis_line(p, cmd)?;
    let angle = req_expr(cmd, p, "angle")?;
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    add_feature(
        s,
        p,
        FeatureKind::ConstructionPlane { plane: solvecraft_doc::PlaneRef::AtAngle { base: Box::new(base), axis_origin, axis_dir, angle } },
    )
}

fn split_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.split_face";
    let faces = face_points(p, cmd)?;
    let body = str_(p, "body").map(str::to_string);
    let xy = || solvecraft_doc::PlaneRef::Origin { name: "XY".into() };
    let kind = if let Some(t) = p.get("tool").filter(|t| !t.is_null()) {
        let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`tool.body` must name a body"))?.to_string();
        let point = t.get("point").and_then(crate::params::vec3).ok_or_else(|| bad(cmd, "`tool.point` must be [x, y, z] on the tool face"))?;
        FeatureKind::SplitFace { faces, body, plane: xy(), tool: Some(solvecraft_doc::FaceAt { body: tb, point }), sketch: None }
    } else if p.get("sketch").is_some_and(|v| !v.is_null()) {
        let sketch = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
        FeatureKind::SplitFace { faces, body, plane: xy(), tool: None, sketch: Some(sketch) }
    } else {
        let plane = plane_param(s, p.get("plane"), cmd)?;
        FeatureKind::SplitFace { faces, body, plane, tool: None, sketch: None }
    };
    add_feature(s, p, kind)
}

fn split_body(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.split_body";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    // A face as the tool: `tool: {body, point}` (its surface extended); else a plane.
    if let Some(t) = p.get("tool").filter(|t| !t.is_null()) {
        let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`tool.body` must name a body"))?.to_string();
        let point = t.get("point").and_then(crate::params::vec3).ok_or_else(|| bad(cmd, "`tool.point` must be [x, y, z] on the tool face"))?;
        let st = s.model.state();
        let tbody = st.body(&tb).ok_or_else(|| bad(cmd, format!("no body `{tb}`")))?;
        if solvecraft_doc::appearance::face_index_at(tbody, point).is_none() {
            return Err(bad(cmd, "`tool.point` is not on a face of the tool body"));
        }
        return add_feature(
            s,
            p,
            FeatureKind::Split {
                body,
                plane: solvecraft_doc::PlaneRef::Origin { name: "XY".into() },
                tool: Some(solvecraft_doc::FaceAt { body: tb, point }),
            },
        );
    }
    let plane = plane_param(s, p.get("plane"), cmd)?;
    add_feature(s, p, FeatureKind::Split { body, plane, tool: None })
}

/// ISO 273 clearance hole diameters (close, normal, loose fit) for metric bolts.
fn clearance_diameter(size: &str, fit: &str) -> Option<f64> {
    const TABLE: [(f64, f64, f64, f64); 21] = [
        (1.6, 1.7, 1.8, 2.0),
        (2.0, 2.2, 2.4, 2.6),
        (2.5, 2.7, 2.9, 3.1),
        (3.0, 3.2, 3.4, 3.6),
        (4.0, 4.3, 4.5, 4.8),
        (5.0, 5.3, 5.5, 5.8),
        (6.0, 6.4, 6.6, 7.0),
        (8.0, 8.4, 9.0, 10.0),
        (10.0, 10.5, 11.0, 12.0),
        (12.0, 13.0, 13.5, 14.5),
        (14.0, 15.0, 15.5, 16.5),
        (16.0, 17.0, 17.5, 18.5),
        (20.0, 21.0, 22.0, 24.0),
        (24.0, 25.0, 26.0, 28.0),
        (30.0, 31.0, 33.0, 35.0),
        (36.0, 37.0, 39.0, 42.0),
        (42.0, 43.0, 45.0, 48.0),
        (48.0, 50.0, 52.0, 56.0),
        (56.0, 58.0, 62.0, 66.0),
        (60.0, 62.0, 66.0, 70.0),
        (64.0, 66.0, 70.0, 74.0),
    ];
    let t = size.trim().trim_start_matches(['M', 'm']);
    let d: f64 = t.split(['x', 'X']).next()?.trim().parse().ok()?;
    let row = TABLE.iter().find(|r| (r.0 - d).abs() < 1e-9)?;
    match fit {
        "close" | "fine" => Some(row.1),
        "normal" | "medium" => Some(row.2),
        "loose" | "coarse" => Some(row.3),
        _ => None,
    }
}

fn hole(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.hole";
    // At sketch points: the holes go perpendicular to the sketch and follow its points.
    let points = match (p.get("sketch"), p.get("points"), p.get("position")) {
        (Some(_), Some(_), _) | (Some(_), None, None) => {
            let sketch = feature_sketch(s, p, cmd)?;
            // No points named: every sketch point (standalone points and circle centres, not
            // line or arc ends, not the origin).
            let ids = if p.get("points").is_some() { string_list(p, "points") } else { sketch_hole_points(s, sketch) };
            if ids.is_empty() || ids.len() > 10_000 {
                return Err(bad(cmd, "`points` must list 1…10000 sketch point ids"));
            }
            let st = s.model.state();
            let ss = st.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
            for id in &ids {
                if !ss.sketch.points.iter().any(|q| &q.id == id) {
                    return Err(bad(cmd, format!("no point `{id}` in the sketch")));
                }
            }
            let n = ss.plane.normal();
            let first =
                ids.first().and_then(|id| ss.sketch.points.iter().find(|q| &q.id == id)).map(|q| ss.plane.to_world(q.pos)).unwrap_or_default();
            Some((solvecraft_doc::SketchPoints { sketch, ids }, first, -n))
        }
        _ => None,
    };
    let thread = str_(p, "thread").map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if let Some(t) = &thread
        && solvecraft_doc::parse_metric_thread(t).is_none()
    {
        return Err(bad(cmd, format!("unknown thread `{t}` (ISO metric, like M6 or M6x0.75)")));
    }
    let position = match (&points, p.get("position").and_then(vec3)) {
        (Some((_, first, _)), None) => *first,
        (_, Some(x)) => x,
        (None, None) => return Err(bad(cmd, "`position` must be [x, y, z] (or give `sketch` and `points`)")),
    };
    let direction = match (p.get("direction").and_then(vec3), &points) {
        (None, Some((_, _, d))) => *d,
        (d, _) => match d {
            Some(d) => d.normalized().ok_or_else(|| bad(cmd, "`direction` must be non-zero"))?,
            None => {
                // Into the planar face the position lies on.
                let st = s.model.state();
                let mut found = None;
                for b in &st.bodies {
                    let tol = (b.body.size() * 1e-3).max(1e-3);
                    for f in b.body.faces(tol).unwrap_or_default() {
                        if let Some(n) = f.plane_normal
                            && (position - f.centroid).dot(n).abs() < tol * 10.0
                        {
                            found = Some(-n);
                        }
                    }
                }
                found.ok_or_else(|| bad(cmd, "no planar face at `position`; give `direction`"))?
            }
        },
    };
    // The diameter: given, or the tap drill of the thread (major − pitch), or an ISO 273
    // clearance hole for a bolt size (close, normal or loose fit).
    let diameter = match (expr(p, "diameter"), &thread, str_(p, "clearance")) {
        (Some(d), _, _) => d,
        (None, _, Some(c)) => {
            let fit = str_(p, "fit").unwrap_or("normal").to_ascii_lowercase();
            let d = clearance_diameter(c, &fit).ok_or_else(|| bad(cmd, format!("no clearance hole for `{c}` ({fit} fit): M1.6 to M64")))?;
            format!("{d} mm")
        }
        (None, Some(t), None) => {
            let (major, pitch) = solvecraft_doc::parse_metric_thread(t).ok_or_else(|| bad(cmd, "thread"))?;
            format!("{} mm", ((major - pitch) * 1000.0).round() / 1000.0)
        }
        (None, None, None) => return Err(bad(cmd, "needs `diameter`, `thread` (a tapped hole) or `clearance` (a bolt size)")),
    };
    check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    let depth = expr(p, "depth");
    if let Some(d) = &depth {
        check_expr(s, d, Kind::Length, cmd, "depth")?;
    }
    let get = |k: &str| -> Result<String> {
        let e = req_expr(cmd, p, k)?;
        Ok(e)
    };
    let kind = match str_(p, "type").unwrap_or("simple") {
        "simple" => match expr(p, "tip_angle") {
            Some(t) if depth.is_some() => solvecraft_doc::HoleKind::Drilled { tip_angle: t },
            _ => solvecraft_doc::HoleKind::Simple,
        },
        "drilled" => solvecraft_doc::HoleKind::Drilled { tip_angle: expr(p, "tip_angle").unwrap_or_else(|| "118 deg".into()) },
        "counterbore" => solvecraft_doc::HoleKind::Counterbore { cb_diameter: get("cb_diameter")?, cb_depth: get("cb_depth")? },
        "countersink" => solvecraft_doc::HoleKind::Countersink {
            cs_diameter: get("cs_diameter")?,
            cs_angle: expr(p, "cs_angle").unwrap_or_else(|| "90 deg".into()),
        },
        other => return Err(bad(cmd, format!("unknown hole type `{other}`"))),
    };
    let to = match p.get("to") {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`to` must be a point [x, y, z] on the face to drill to"))?),
        None => None,
    };
    add_feature(s, p, FeatureKind::Hole { position, direction, diameter, depth, hole: kind, points: points.map(|x| x.0), thread, to })
}

/// The points of a sketch a hole drills at when only the sketch is given.
fn sketch_hole_points(s: &Session, sketch: u64) -> Vec<String> {
    let st = s.model.state();
    let Some(ss) = st.sketch(sketch) else { return Vec::new() };
    let mut ends: Vec<usize> = Vec::new();
    for c in &ss.sketch.curves {
        match &c.kind {
            solvecraft_sketch::CurveKind::Circle { .. } => {}
            solvecraft_sketch::CurveKind::Arc { a, b, .. } | solvecraft_sketch::CurveKind::Line { a, b } => ends.extend([*a, *b]),
            solvecraft_sketch::CurveKind::Ellipse { m, .. } => ends.push(*m),
            solvecraft_sketch::CurveKind::Spline { pts, .. } => ends.extend(pts.iter().copied()),
            solvecraft_sketch::CurveKind::Conic { a, b, apex, .. } => ends.extend([*a, *b, *apex]),
        }
    }
    ss.sketch.points.iter().enumerate().filter(|(i, p)| p.id != "origin" && !ends.contains(i)).map(|(_, p)| p.id.clone()).collect()
}

fn face_points(p: &Value, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get("faces").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`faces` must be a list of points"))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, "select 1…1000 faces"));
    }
    a.iter().map(|v| vec3(v).or_else(|| v.get("point").and_then(vec3)).ok_or_else(|| bad(cmd, "a face is a point [x, y, z] on it"))).collect()
}

fn shell(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.shell";
    let faces = face_points(p, cmd)?;
    let thickness = req_expr(cmd, p, "thickness")?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let direction = str_(p, "direction").map(str::to_ascii_lowercase).unwrap_or_default();
    if !matches!(direction.as_str(), "" | "inside" | "outside" | "both") {
        return Err(bad(cmd, "`direction` must be inside, outside or both"));
    }
    add_feature(
        s,
        p,
        FeatureKind::Shell {
            faces,
            thickness,
            body: str_(p, "body").map(str::to_string),
            direction: if direction == "inside" { String::new() } else { direction },
            tangent_chain: bool_(p, "tangent_chain").unwrap_or(false),
        },
    )
}

fn draft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.draft";
    let faces = face_points(p, cmd)?;
    let angle = req_expr(cmd, p, "angle")?;
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let neutral = plane_param(s, p.get("neutral"), cmd)?;
    let (vals, _) = s.doc.param_values();
    let pl = s.doc.resolve_plane_in(&vals, &neutral, 0, Some(&s.model.state()))?;
    let pull = p.get("pull").and_then(vec3).unwrap_or_else(|| pl.normal());
    add_feature(s, p, FeatureKind::Draft { faces, angle, neutral, pull, body: str_(p, "body").map(str::to_string) })
}

pub(super) fn sketch_id(s: &Session, v: Option<&Value>, cmd: &str, what: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, format!("`{what}` must be a sketch id or name"))),
    };
    match s.doc.find_feature(&key) {
        Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => Ok(f.id),
        _ => Err(bad(cmd, format!("no sketch `{key}`"))),
    }
}

fn sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.sweep";
    let sketch = feature_sketch(s, p, cmd)?;
    let path_sketch = sketch_id(s, p.get("path_sketch"), cmd, "path_sketch")?;
    let path = string_list(p, "path");
    if path.is_empty() {
        return Err(bad(cmd, "`path` must list curve ids"));
    }
    add_feature(
        s,
        p,
        FeatureKind::Sweep {
            sketch,
            profiles: profiles(p, cmd)?,
            path_sketch,
            path,
            operation: operation(p, cmd)?,
            targets: string_list(p, "targets"),
        },
    )
}

fn loft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.loft";
    let list = p.get("sections").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`sections` must be a list"))?;
    if list.len() < 2 || list.len() > 50 {
        return Err(bad(cmd, "a loft needs 2…50 sections"));
    }
    let mut sections = Vec::new();
    for sec in list {
        let sketch = sketch_id(s, sec.get("sketch"), cmd, "sections[].sketch")?;
        // A sketch point closes the loft to an apex.
        if let Some(pt) = str_(sec, "point") {
            let st = s.model.state();
            if !st.sketch(sketch).is_some_and(|ss| ss.sketch.points.iter().any(|q| q.id == pt)) {
                return Err(bad(cmd, format!("no point `{pt}` in the section's sketch")));
            }
            sections.push(solvecraft_doc::LoftSection { sketch, profiles: solvecraft_doc::ProfileSel::All, point: Some(pt.to_string()) });
            continue;
        }
        sections.push(solvecraft_doc::LoftSection { sketch, profiles: profiles(sec, cmd)?, point: None });
    }
    add_feature(s, p, FeatureKind::Loft { sections, operation: operation(p, cmd)?, targets: string_list(p, "targets") })
}

fn thread(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.thread";
    let face = p
        .get("face")
        .and_then(|v| vec3(v).or_else(|| v.get("point").and_then(vec3)))
        .ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on a cylindrical face"))?;
    let designation = str_(p, "designation").map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if let Some(t) = &designation
        && solvecraft_doc::parse_metric_thread(t).is_none()
    {
        return Err(bad(cmd, format!("unknown thread `{t}` (ISO metric, like M8 or M8x1)")));
    }
    let length = expr(p, "length");
    if let Some(l) = &length {
        check_expr(s, l, Kind::Length, cmd, "length")?;
    }
    let modeled = p.get("modeled").and_then(Value::as_bool).unwrap_or(false);
    let v = add_feature(s, p, FeatureKind::Thread { face, designation, length, modeled })?;
    let st = s.model.state();
    let t = v.get("feature").and_then(Value::as_u64).and_then(|id| st.threads.iter().find(|t| t.feature == id).cloned());
    Ok(json!({"feature": v.get("feature"), "name": v.get("name"), "thread": t}))
}

fn pipe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.pipe";
    let path_sketch = sketch_id(s, p.get("path_sketch"), cmd, "path_sketch")?;
    let path = string_list(p, "path");
    if path.is_empty() {
        return Err(bad(cmd, "`path` must list curve ids"));
    }
    let diameter = req_expr(cmd, p, "diameter")?;
    check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    let wall = expr(p, "wall");
    if let Some(w) = &wall {
        check_expr(s, w, Kind::Length, cmd, "wall")?;
    }
    add_feature(s, p, FeatureKind::Pipe { path_sketch, path, diameter, wall, operation: operation(p, cmd)?, targets: string_list(p, "targets") })
}

fn bounding_solid(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.bounding_solid";
    let margin = expr(p, "margin").unwrap_or_else(|| "0".into());
    check_expr(s, &margin, Kind::Length, cmd, "margin")?;
    add_feature(s, p, FeatureKind::BoundingSolid { bodies: string_list(p, "bodies"), margin })
}

fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.scale";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let factors = match p.get("factors").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => {
            let e: Vec<String> = a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect();
            for x in &e {
                check_expr(s, x, Kind::Unitless, cmd, "factors")?;
            }
            Some([e[0].clone(), e[1].clone(), e[2].clone()])
        }
        Some(_) => return Err(bad(cmd, "`factors` must have three values")),
        None => None,
    };
    let factor = match (&factors, expr(p, "factor")) {
        (_, Some(f)) => f,
        (Some(_), None) => "1".into(),
        (None, None) => return Err(bad(cmd, "give `factor` or `factors`")),
    };
    check_expr(s, &factor, Kind::Unitless, cmd, "factor")?;
    let origin = p.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
    add_feature(s, p, FeatureKind::Scale { bodies, origin, factor, factors })
}

fn delete_face(s: &mut Session, p: &Value) -> Result<Value> {
    let faces = face_points(p, "face.delete")?;
    add_feature(s, p, FeatureKind::DeleteFace { faces, body: str_(p, "body").map(str::to_string) })
}

fn offset_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.offset_face";
    let faces = face_points(p, cmd)?;
    let distance = req_expr(cmd, p, "distance")?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    add_feature(s, p, FeatureKind::OffsetFace { faces, distance, body: str_(p, "body").map(str::to_string) })
}

#[cfg(test)]
#[path = "patterns_tests.rs"]
mod patterns_tests;
