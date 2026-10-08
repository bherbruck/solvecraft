//! Inspection: measure bodies, describe the document, sketches and model topology.

use serde_json::{Value, json};
use solvecraft_sketch::CurveKind;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("MeasureCommand", "Measure", measure)
        .at("SOLID", "INSPECT")
        .icon("measure")
        .key("I")
        .noundo()
        .params("bodies?: [names] (default all); or items: [1-2 selections] → length/area/volume, minimum distance (from, to) and angle"),
    CommandSpec::new("document.inspect", "Inspect Design", inspect).noundo().params("measure?: bool (include body measurements)"),
    CommandSpec::new("sketch.inspect", "Inspect Sketch", sketch_inspect).noundo().params("sketch?: id|name (default active)"),
    CommandSpec::new("model.edges", "List Edges", model_edges).noundo().params("body: name"),
    CommandSpec::new("model.faces", "List Faces", model_faces).noundo().params("body: name"),
    CommandSpec::new("model.threads", "List Threads", model_threads).noundo(),
    CommandSpec::new("PhysicalMaterialCommand", "Physical Material", physical_material)
        .at("SOLID", "MODIFY")
        .icon("material")
        .params("bodies: [names]; material: name (Steel, Aluminum, ABS Plastic, …; `material.list`)"),
    CommandSpec::new("AppearanceCommand", "Appearance", appearance).at("SOLID", "MODIFY").icon("appearance").key("A").params(
        "bodies?: [names]; faces?: [[x,y,z] on faces] (body?: which body, default the nearest); components?: [names or ids, or occurrence names]; \
             appearance?: library name (appearance.library) | color: \"#rrggbb\" or [r, g, b] (0–255), opacity? (0–1, default 1); \
             neither (or color: null): clear, so the face follows its body, the body its component, the component its material",
    ),
    CommandSpec::new("appearance.library", "Appearance Library", appearance_library)
        .noundo()
        .params("→ the built-in appearances (name, colour, opacity)"),
    CommandSpec::new("appearance.list", "List Appearances", appearance_list)
        .noundo()
        .params("body?: name → assigned appearances; with body, its resolved look and its faces' (face index, look)"),
    CommandSpec::new("material.list", "List Materials", material_list).noundo(),
    CommandSpec::new("engine.commands", "List Commands", commands).noundo(),
];

pub fn measure_json(b: &solvecraft_doc::ModelBody) -> Value {
    match solvecraft_kernel::measure(&b.body) {
        Ok(m) => json!({
            "name": b.name,
            "volume_mm3": m.volume,
            "area_mm2": m.area,
            "center_of_mass": m.centroid,
            "bbox": {"min": m.bbox.min, "max": m.bbox.max},
            "faces": m.merged.faces,
            "edges": m.merged.edges,
            "vertices": m.merged.vertices,
            "face_types": m.merged.face_types,
            "kernel": {"faces": m.faces, "edges": m.edges, "vertices": m.vertices, "shells": m.shells},
        }),
        Err(e) => json!({"name": b.name, "error": e.to_string()}),
    }
}

fn measure(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(items) = p.get("items") {
        return super::measure_sel::measure_items(s, items);
    }
    let want = string_list(p, "bodies");
    let st = s.world_state();
    let mut bodies: Vec<Value> = st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).map(measure_json).collect();
    // Mass from each body's material.
    for b in &mut bodies {
        let name = b["name"].as_str().unwrap_or_default().to_string();
        if let Some(v) = b["volume_mm3"].as_f64() {
            let rho = s.doc.density(&name);
            b["material"] = json!(s.doc.materials.get(&name).cloned().unwrap_or_else(|| "Default".into()));
            b["density_g_cm3"] = json!(rho);
            b["mass_g"] = json!(v * rho / 1000.0);
        }
    }
    if !want.is_empty() && bodies.len() != want.len() {
        return Err(bad("MeasureCommand", "unknown body name"));
    }
    // Sheet metal keeps the edges where bends meet flat faces (as Fusion does): count its
    // B-rep faces as they are, without merging coplanar neighbours.
    let sheets = s.model.state().sheets.clone();
    for b in &mut bodies {
        if let Some(sh) = sheets.iter().find(|x| b["name"].as_str() == Some(x.body.as_str())) {
            let (df, de, dv) = sh.round_hole_correction();
            for (k, d) in [("faces", df), ("edges", de), ("vertices", dv)] {
                b[k] = json!(b["kernel"][k].as_i64().unwrap_or(0) + d);
            }
            b["sheet_metal"] = json!(true);
        }
    }
    let tv: f64 = bodies.iter().filter_map(|b| b["volume_mm3"].as_f64()).sum();
    let ta: f64 = bodies.iter().filter_map(|b| b["area_mm2"].as_f64()).sum();
    let tf: u64 = bodies.iter().filter_map(|b| b["faces"].as_u64()).sum();
    let te: u64 = bodies.iter().filter_map(|b| b["edges"].as_u64()).sum();
    let tvx: u64 = bodies.iter().filter_map(|b| b["vertices"].as_u64()).sum();
    let tm: f64 = bodies.iter().filter_map(|b| b["mass_g"].as_f64()).sum();
    Ok(
        json!({"body_count": bodies.len(), "bodies": bodies, "total": {"volume_mm3": tv, "area_mm2": ta, "mass_g": tm, "faces": tf, "edges": te, "vertices": tvx}}),
    )
}

fn sketch_json(s: &Session, id: u64) -> Option<Value> {
    let state = s.model.state();
    let ss = state.sketch(id)?;
    let sk = &ss.sketch;
    let curves: Vec<Value> = sk
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let det = ss.report.curve_determined.get(i).copied().unwrap_or(false);
            let pid = |k: usize| sk.points.get(k).map(|p| p.id.clone()).unwrap_or_default();
            let mut v = match c.kind {
                CurveKind::Line { a, b } => json!({"id": c.id, "type": "line", "start": pid(a), "end": pid(b), "start_at": sk.point(a), "end_at": sk.point(b), "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Circle { c: cc, r } => json!({"id": c.id, "type": "circle", "center": pid(cc), "center_at": sk.point(cc), "radius": r, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Arc { c: cc, a, b } => json!({"id": c.id, "type": "arc", "center": pid(cc), "start": pid(a), "end": pid(b), "center_at": sk.point(cc), "start_at": sk.point(a), "end_at": sk.point(b), "radius": sk.radius(i), "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Ellipse { c: cc, m, r } => json!({"id": c.id, "type": "ellipse", "center": pid(cc), "major": pid(m), "center_at": sk.point(cc), "major_at": sk.point(m), "minor_radius": r, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Spline { ref pts, control, degree } => json!({"id": c.id, "type": "spline", "points": pts.iter().map(|q| pid(*q)).collect::<Vec<_>>(), "points_at": pts.iter().map(|q| sk.point(*q)).collect::<Vec<_>>(), "control": control, "degree": degree, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Conic { a, b, apex, rho } => json!({"id": c.id, "type": "conic", "start": pid(a), "end": pid(b), "apex": pid(apex), "start_at": sk.point(a), "end_at": sk.point(b), "apex_at": sk.point(apex), "rho": rho, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
            };
            if let (Some(l), Some(o)) = (&c.link, v.as_object_mut()) {
                o.insert("link".into(), json!(l));
            }
            v
        })
        .collect();
    let locked = sk.locked_points();
    let points: Vec<Value> = sk
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"id": p.id, "at": p.pos, "world": ss.plane.to_world(p.pos), "fixed": locked.get(i).copied().unwrap_or(false), "link": p.link, "fully_constrained": ss.report.point_determined.get(i).copied().unwrap_or(false)}))
        .collect();
    let constraints: Vec<Value> = sk
        .constraints
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c.kind).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(c.id));
                o.insert("name".into(), json!(c.kind.name()));
                if let Some(p) = &c.param {
                    o.insert("param".into(), json!(p));
                    o.insert("expression".into(), json!(s.doc.param(p).map(|x| x.expr.clone())));
                }
            }
            v
        })
        .collect();
    let profiles: Vec<Value> = ss
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"index": i, "area_mm2": p.area, "centroid_sketch": p.centroid, "curves": p.outer_curves, "holes": p.hole_curves}))
        .collect();
    Some(json!({
        "id": id,
        "name": ss.name,
        "plane": ss.plane,
        "status": ss.report.status,
        "dof": ss.report.dof,
        "fully_constrained": ss.report.fully_constrained(),
        "failing": ss.report.failing,
        "curves": curves,
        "points": points,
        "constraints": constraints,
        "profiles": profiles,
        "links": sk.links,
        "wires": sk.wires.iter().map(|w| json!({"id": w.id, "points": w.pts.len(), "start": w.pts.first(), "end": w.pts.last(), "link": w.link})).collect::<Vec<_>>(),
    }))
}

fn sketch_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match p.get("sketch") {
        Some(Value::Number(n)) => s.doc.find_feature(&n.to_string()).map(|f| f.id),
        Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
        _ => s.active_sketch,
    }
    .ok_or_else(|| bad("sketch.inspect", "no such sketch (and none is active)"))?;
    sketch_json(s, id).ok_or_else(|| EngineError::Other("that sketch is not evaluated (rolled back or not a sketch)".into()))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let (vals, perr) = s.doc.param_values();
    let params: Vec<Value> = s
        .doc
        .params
        .iter()
        .map(|p| json!({"name": p.name, "expression": p.expr, "unit": p.unit, "value": vals.get(&p.name).map(|v| v.v), "model": p.model, "comment": p.comment, "error": perr.get(&p.name)}))
        .collect();
    let marker = s.doc.marker.unwrap_or(s.doc.features.len());
    let timeline: Vec<Value> = s
        .doc
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let r = s.model.result(f.id);
            json!({
                "id": f.id, "name": f.name, "type": f.kind.type_name(), "suppressed": f.suppressed, "rolled_back": i >= marker,
                "error": r.and_then(|r| r.error.clone()), "warning": r.and_then(|r| r.warning.clone()), "ms": r.map(|r| r.ms),
            })
        })
        .collect();
    let with_measure = bool_(p, "measure").unwrap_or(false);
    let bodies: Vec<Value> =
        st.bodies.iter().map(|b| if with_measure { measure_json(b) } else { json!({"name": b.name, "feature": b.feature}) }).collect();
    let sketches: Vec<Value> = st
        .sketches
        .iter()
        .map(|ss| json!({"id": ss.feature, "name": ss.name, "dof": ss.report.dof, "status": ss.report.status, "curves": ss.sketch.curves.len(), "profiles": ss.profiles.len()}))
        .collect();
    Ok(json!({
        "name": s.doc.name, "units": s.doc.units, "path": s.path, "dirty": s.is_dirty(), "revision": s.revision,
        "active_sketch": s.active_sketch, "marker": marker,
        "params": params, "timeline": timeline, "bodies": bodies, "sketches": sketches,
        "selection": s.selection, "undo": s.undo.len(), "redo": s.redo.len(),
    }))
}

fn body_of<'a>(st: &'a solvecraft_doc::ModelState, p: &Value, cmd: &str) -> Result<&'a solvecraft_doc::ModelBody> {
    let n = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?;
    st.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))
}

fn model_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state();
    let b = body_of(&st, p, "model.edges")?;
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let names = solvecraft_doc::naming::edge_names(b);
    let edges: Vec<Value> = b
        .body
        .edges(tol)?
        .iter()
        .map(|e| json!({"index": e.index, "name": names.get(e.index), "mid": e.mid, "length": e.length, "start": e.points.first(), "end": e.points.last()}))
        .collect();
    Ok(json!({"body": b.name, "edges": edges}))
}

fn model_faces(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state();
    let b = body_of(&st, p, "model.faces")?;
    let names = solvecraft_doc::naming::face_names(b);
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let faces: Vec<Value> = b
        .body
        .faces(tol)?
        .iter()
        .map(|f| json!({"index": f.index, "name": names.get(f.index), "area": f.area, "centroid": f.centroid, "plane_normal": f.plane_normal}))
        .collect();
    Ok(json!({"body": b.name, "faces": faces}))
}

fn commands(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(crate::command_specs().iter().map(|c| serde_json::to_value(c.info(s)).unwrap_or_default()).collect()))
}

fn model_threads(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.world_state();
    Ok(json!({ "threads": st.threads }))
}

fn physical_material(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "PhysicalMaterialCommand";
    let bodies = string_list(p, "bodies");
    let material = str_(p, "material").ok_or_else(|| bad(cmd, "`material` must be a material name"))?;
    let canonical = solvecraft_doc::MATERIALS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(material.trim()))
        .map(|(n, _)| n.to_string())
        .ok_or_else(|| bad(cmd, format!("unknown material `{material}` (see material.list)")))?;
    let st = s.model.state();
    if bodies.is_empty() || bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    for b in &bodies {
        if canonical == "Default" {
            s.doc_mut().materials.remove(b);
        } else {
            s.doc_mut().materials.insert(b.clone(), canonical.clone());
        }
    }
    Ok(json!({"bodies": bodies, "material": canonical}))
}

/// A colour parameter: `"#rrggbb"` or `[r, g, b]` (0–255).
fn color_param(v: &Value) -> Option<[u8; 3]> {
    if let Some(t) = v.as_str() {
        let h = t.trim().trim_start_matches('#');
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        return Some([byte(0)?, byte(2)?, byte(4)?]);
    }
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite() && (0.0..=255.0).contains(x)).map(|x| x.round() as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

fn appearance(s: &mut Session, p: &Value) -> Result<Value> {
    use solvecraft_doc::appearance::{FaceLook, Look, library};
    let cmd = "AppearanceCommand";
    let look: Option<Look> = match (str_(p, "appearance"), p.get("color")) {
        (Some(n), _) => {
            let mut l = library(n).ok_or_else(|| bad(cmd, format!("no appearance `{n}` in the library (appearance.library)")))?;
            if let Some(o) = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
                l.opacity = o.clamp(0.0, 1.0);
            }
            Some(l)
        }
        (None, None | Some(Value::Null)) => None,
        (None, Some(v)) => {
            let c = color_param(v).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?;
            let o = match p.get("opacity") {
                Some(o) => o.as_f64().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or_else(|| bad(cmd, "`opacity` must be 0…1"))?,
                None => 1.0,
            };
            Some(Look::custom(c, o))
        }
    };
    let bodies = string_list(p, "bodies");
    let st = s.model.state();
    if bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    // Faces: a point on each, on the named body or the nearest one.
    let mut faces: Vec<(String, solvecraft_geom::Vec3)> = Vec::new();
    if let Some(list) = p.get("faces") {
        let list = list.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
        for v in list {
            let pt = crate::params::vec3(v)
                .or_else(|| v.get("point").and_then(crate::params::vec3))
                .ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
            let on = |b: &solvecraft_doc::ModelBody| solvecraft_doc::appearance::face_index_at(b, pt).is_some();
            let body = match v.get("body").and_then(Value::as_str).or_else(|| str_(p, "body")) {
                Some(n) => st.body(n).filter(|b| on(b)).map(|b| b.name.clone()),
                None => st.bodies.iter().filter(|b| on(b)).min_by(|a, b| dist_to(a, pt).total_cmp(&dist_to(b, pt))).map(|b| b.name.clone()),
            };
            faces.push((body.ok_or_else(|| bad(cmd, format!("no face at {pt:?}")))?, pt));
        }
    }
    let mut comps: Vec<u64> = Vec::new();
    for c in string_list(p, "components") {
        let id = s
            .doc
            .find_component(&c)
            .or_else(|| s.doc.occurrences.iter().find(|o| o.name == c).map(|o| o.component))
            .filter(|id| *id != 0)
            .ok_or_else(|| bad(cmd, format!("no component `{c}`")))?;
        comps.push(id);
    }
    if bodies.is_empty() && faces.is_empty() && comps.is_empty() {
        return Err(bad(cmd, "give `bodies`, `faces` or `components`"));
    }
    let a = &mut s.doc_mut().appearances;
    for b in &bodies {
        match &look {
            Some(l) => a.bodies.insert(b.clone(), l.clone()),
            None => a.bodies.remove(b),
        };
    }
    for (body, pt) in &faces {
        // One entry per face: a new one replaces any at the same spot.
        a.faces.retain(|f| !(f.body == *body && f.point.dist(*pt) < 1e-9));
        if let Some(l) = &look {
            if a.faces.len() >= 100_000 {
                return Err(bad(cmd, "too many face appearances"));
            }
            a.faces.push(FaceLook { body: body.clone(), point: *pt, look: l.clone() });
        }
    }
    for c in &comps {
        match &look {
            Some(l) => a.components.insert(*c, l.clone()),
            None => a.components.remove(c),
        };
    }
    // Clearing a face: drop every entry on that face, not just at the same point.
    if look.is_none() && !faces.is_empty() {
        let st = s.model.state();
        let doc = s.doc_mut();
        let picked: Vec<(String, Option<usize>)> =
            faces.iter().map(|(b, pt)| (b.clone(), st.body(b).and_then(|mb| solvecraft_doc::appearance::face_index_at(mb, *pt)))).collect();
        doc.appearances.faces.retain(|f| {
            let idx = st.body(&f.body).and_then(|mb| solvecraft_doc::appearance::face_index_at(mb, f.point));
            !picked.iter().any(|(b, i)| *b == f.body && i.is_some() && *i == idx)
        });
    }
    Ok(json!({
        "bodies": bodies,
        "faces": faces.iter().map(|(b, p)| json!({"body": b, "point": p})).collect::<Vec<_>>(),
        "components": comps,
        "appearance": look.as_ref().map(|l| json!({"name": l.name, "color": l.hex(), "opacity": l.opacity})),
    }))
}

fn dist_to(b: &solvecraft_doc::ModelBody, p: solvecraft_geom::Vec3) -> f64 {
    let bb = b.mesh().bounds();
    let c = solvecraft_geom::Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z));
    c.dist(p)
}

fn look_json(l: &solvecraft_doc::appearance::Look) -> Value {
    json!({"name": l.name, "color": l.hex(), "opacity": l.opacity})
}

fn appearance_library(_s: &mut Session, _p: &Value) -> Result<Value> {
    let list: Vec<Value> = solvecraft_doc::appearance::LIBRARY
        .iter()
        .map(|(n, c, o)| {
            let category = n.split(" - ").next().unwrap_or(n);
            json!({"name": n, "category": category, "color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), "opacity": o})
        })
        .collect();
    Ok(json!({"appearances": list}))
}

fn appearance_list(s: &mut Session, p: &Value) -> Result<Value> {
    let a = &s.doc.appearances;
    // "In this design": each look used, with where.
    let mut used: Vec<(solvecraft_doc::appearance::Look, Vec<Value>, Vec<Value>, Vec<Value>)> = Vec::new();
    fn slot(used: &mut Vec<(solvecraft_doc::appearance::Look, Vec<Value>, Vec<Value>, Vec<Value>)>, l: &solvecraft_doc::appearance::Look) -> usize {
        match used.iter().position(|(u, ..)| u == l) {
            Some(i) => i,
            None => {
                used.push((l.clone(), Vec::new(), Vec::new(), Vec::new()));
                used.len() - 1
            }
        }
    }
    for (b, l) in &a.bodies {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.1.push(json!(b));
        }
    }
    for f in &a.faces {
        let i = slot(&mut used, &f.look);
        if let Some(u) = used.get_mut(i) {
            u.2.push(json!({"body": f.body, "point": f.point}));
        }
    }
    for (c, l) in &a.components {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.3.push(json!(c));
        }
    }
    let in_design: Vec<Value> = used
        .iter()
        .map(|(l, b, f, c)| json!({"name": l.name, "color": l.hex(), "opacity": l.opacity, "bodies": b, "faces": f, "components": c}))
        .collect();
    let mut out = json!({
        "in_design": in_design,
        "bodies": a.bodies.iter().map(|(b, l)| (b.clone(), look_json(l))).collect::<serde_json::Map<_, _>>(),
        "faces": a.faces.iter().map(|f| json!({"body": f.body, "point": f.point, "appearance": look_json(&f.look)})).collect::<Vec<_>>(),
        "components": a.components.iter().map(|(c, l)| json!({"component": c, "appearance": look_json(l)})).collect::<Vec<_>>(),
    });
    if let Some(name) = str_(p, "body") {
        let st = s.model.state();
        let b = st.body(name).ok_or_else(|| bad("appearance.list", format!("no body `{name}`")))?;
        out["body"] = json!({
            "name": name,
            "appearance": s.doc.body_look(&b.name, b.feature).map(|l| look_json(&l)),
            "color": s.doc.body_color(b),
            "faces": s.doc.face_colors(b).iter().map(|(i, l)| json!({"face": i, "appearance": look_json(l)})).collect::<Vec<_>>(),
        });
    }
    Ok(out)
}

fn material_list(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"materials": solvecraft_doc::MATERIALS.iter().map(|(n, d)| json!({"name": n, "density_g_cm3": d})).collect::<Vec<_>>()}))
}
