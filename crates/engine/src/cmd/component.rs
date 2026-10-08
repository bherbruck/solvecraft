//! Components and occurrences: the design as a tree (see `solvecraft_doc` assembly docs).
//! New sketches and features go into the active component, authored in its frame; bodies
//! belong to their feature's component unless moved; occurrences place components (instances
//! share contents, "paste new" copies them) and can be grounded and moved.

use serde_json::{Value, json};
use solvecraft_doc::{FeatureKind, Mat, Occurrence};
use solvecraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, string_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FusionCreateNewComponentCommand", "New Component", new_component)
        .at("SOLID", "ASSEMBLE")
        .icon("component")
        .params("name?, parent?: component id or name (default: the active one); activate?: bool (default true)"),
    CommandSpec::new("FusionCreateComponentsFromBodiesCommand", "Create Components from Bodies", from_bodies)
        .icon("component")
        .params("bodies: [names] → one new component per body, named after it"),
    CommandSpec::new("component.activate", "Activate Component", activate).noundo().params("component: id or name (0 or \"root\" for the design)"),
    CommandSpec::new("component.list", "List Components", list).noundo(),
    CommandSpec::new("component.rename", "Rename Component", rename).params("component, name"),
    CommandSpec::new("component.delete", "Delete Component", delete).params("component: deletes it, its subcomponents, occurrences and features"),
    CommandSpec::new("component.move_bodies", "Move Bodies to Component", move_bodies).params("bodies: [names], component"),
    CommandSpec::new("component.move_sketches", "Move Sketches to Component", move_sketches).params("sketches: [ids or names], component"),
    CommandSpec::new("occurrence.copy", "Paste (Instance)", copy_occurrence)
        .params("component (or occurrence): what to place again; parent?: component (default: the same parent); translate?: [x,y,z]"),
    CommandSpec::new("component.paste_new", "Paste New", paste_new).params("component: copied with its features into a new, independent component; translate?: [x,y,z]"),
    CommandSpec::new("occurrence.move", "Move Occurrence", move_occurrence)
        .params("occurrence; translate?: [x,y,z]; axis?: [x,y,z], angle?: radians or \"30 deg\", origin?: [x,y,z]; capture?: bool (default true; false keeps it pending until Capture Position)"),
    CommandSpec::new("occurrence.ground", "Ground", ground).params("occurrence; grounded?: bool (default toggles)"),
    CommandSpec::new("SnapshotCmd", "Capture Position", capture).at("SOLID", "POSITION").icon("capture").params("keeps pending occurrence moves"),
    CommandSpec::new("AsBuiltPositionsCmd", "Revert Position", revert).at("SOLID", "POSITION").icon("revert").noundo().params("drops pending occurrence moves"),
];

fn key(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn component(s: &Session, p: &Value, k: &str, cmd: &str) -> Result<u64> {
    let kk = key(p.get(k)).ok_or_else(|| bad(cmd, format!("`{k}` must be a component id or name")))?;
    s.doc.find_component(&kk).ok_or_else(|| bad(cmd, format!("no component `{kk}`")))
}

fn occurrence(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let kk = key(p.get("occurrence")).ok_or_else(|| bad(cmd, "`occurrence` must be an id or name"))?;
    s.doc.occurrences.iter().find(|o| o.id.to_string() == kk || o.name == kk).map(|o| o.id).ok_or_else(|| bad(cmd, format!("no occurrence `{kk}`")))
}

fn translation(t: Vec3) -> Mat {
    solvecraft_doc::rigid(t, Vec3::ZERO, Vec3::Z, 0.0)
}

fn new_component(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCreateNewComponentCommand";
    let parent = if p.get("parent").is_some() { component(s, p, "parent", cmd)? } else { s.active_component };
    let id = s.doc_mut().add_component(str_(p, "name"), parent)?;
    if p.get("activate").and_then(Value::as_bool).unwrap_or(true) {
        s.active_component = id;
    }
    let name = s.doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    let occ = s.doc.occurrence_of(id).map(|o| o.id);
    Ok(json!({"component": id, "name": name, "parent": parent, "occurrence": occ}))
}

fn from_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCreateComponentsFromBodiesCommand";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() || bodies.len() > 1000 {
        return Err(bad(cmd, "`bodies` must list 1…1000 body names"));
    }
    let st = s.model.state();
    for b in &bodies {
        if st.body(b).is_none() {
            return Err(bad(cmd, format!("no body `{b}`")));
        }
    }
    let parent = s.active_component;
    let mut made = Vec::new();
    for b in bodies {
        let id = s.doc_mut().add_component(Some(&b), parent)?;
        s.doc_mut().body_components.insert(b.clone(), id);
        made.push(json!({"component": id, "body": b}));
    }
    Ok(json!({ "components": made }))
}

fn activate(s: &mut Session, p: &Value) -> Result<Value> {
    let id = component(s, p, "component", "component.activate")?;
    s.active_component = id;
    s.revision += 1;
    Ok(json!({ "active": id }))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.model.state();
    let doc = &s.doc;
    let node = |id: u64, name: &str, parent: Option<u64>| {
        let bodies: Vec<&str> = st.bodies.iter().filter(|b| doc.body_component(&b.name, b.feature) == id).map(|b| b.name.as_str()).collect();
        let features: Vec<&str> = doc
            .features
            .iter()
            .filter(|f| f.component == id && !matches!(f.kind, FeatureKind::Sketch { .. } | FeatureKind::ConstructionPlane { .. }))
            .map(|f| f.name.as_str())
            .collect();
        let sketches: Vec<&str> =
            doc.features.iter().filter(|f| f.component == id && matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.name.as_str()).collect();
        let construction: Vec<&str> = doc
            .features
            .iter()
            .filter(|f| f.component == id && matches!(f.kind, FeatureKind::ConstructionPlane { .. }))
            .map(|f| f.name.as_str())
            .collect();
        let occurrences: Vec<&Occurrence> = doc.occurrences.iter().filter(|o| o.component == id).collect();
        let children: Vec<&Occurrence> = doc.occurrences.iter().filter(|o| o.parent == id).collect();
        json!({"id": id, "name": name, "parent": parent, "bodies": bodies, "features": features, "sketches": sketches,
            "construction": construction, "occurrences": occurrences, "children": children})
    };
    let mut out = vec![node(0, &doc.name, None)];
    for c in &doc.components {
        out.push(node(c.id, &c.name, Some(c.parent)));
    }
    Ok(json!({"active": s.active_component, "components": out, "pending_moves": s.pending_moves.keys().collect::<Vec<_>>()}))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.rename";
    let id = component(s, p, "component", cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if id == 0 {
        s.doc_mut().name = name.to_string();
    } else if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) {
        c.name = name.to_string();
    }
    Ok(json!({"component": id, "name": name}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.delete";
    let id = component(s, p, "component", cmd)?;
    if id == 0 {
        return Err(bad(cmd, "the design itself can't be deleted"));
    }
    let gone: Vec<u64> = s.doc.components.iter().filter(|c| s.doc.component_within(c.id, id)).map(|c| c.id).collect();
    let feats: Vec<u64> = s.doc.features.iter().filter(|f| gone.contains(&f.component)).map(|f| f.id).collect();
    let d = s.doc_mut();
    for f in &feats {
        let _ = d.delete_feature(*f);
    }
    d.components.retain(|c| !gone.contains(&c.id));
    d.occurrences.retain(|o| !gone.contains(&o.component) && !gone.contains(&o.parent));
    let moved_out: Vec<String> = d.body_components.iter().filter(|(_, c)| gone.contains(c)).map(|(b, _)| b.clone()).collect();
    d.body_components.retain(|_, c| !gone.contains(c));
    d.body_offsets.retain(|b, _| !moved_out.contains(b));
    if gone.contains(&s.active_component) {
        s.active_component = 0;
    }
    Ok(json!({"deleted": gone, "features": feats}))
}

fn move_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.move_bodies";
    let to = component(s, p, "component", cmd)?;
    let bodies = string_list(p, "bodies");
    let st = s.model.state();
    if bodies.is_empty() || bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    for b in &bodies {
        let feature = st.body(b).map(|x| x.feature).unwrap_or(0);
        let own = s.doc.feature(feature).map(|f| f.component).unwrap_or(0);
        // The body keeps its place in the world: where it is shown now (its component's frame and
        // any earlier offset), seen from the new component's frame.
        let from = s.doc.body_component(b, feature);
        let now = solvecraft_doc::mat_mul(&s.doc.component_transform(from), s.doc.body_offsets.get(b).unwrap_or(&solvecraft_doc::IDENTITY));
        let offset = solvecraft_doc::mat_inverse(&s.doc.component_transform(to))
            .map(|inv| solvecraft_doc::mat_mul(&inv, &now))
            .unwrap_or(solvecraft_doc::IDENTITY);
        let doc = s.doc_mut();
        if own == to {
            doc.body_components.remove(b);
        } else {
            doc.body_components.insert(b.clone(), to);
        }
        if solvecraft_doc::is_identity(&offset) {
            doc.body_offsets.remove(b);
        } else {
            doc.body_offsets.insert(b.clone(), offset);
        }
    }
    Ok(json!({"bodies": bodies, "component": to}))
}

fn move_sketches(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.move_sketches";
    let to = component(s, p, "component", cmd)?;
    let keys = string_list(p, "sketches");
    let mut ids = Vec::new();
    for k in &keys {
        match s.doc.find_feature(k) {
            Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => ids.push(f.id),
            _ => return Err(bad(cmd, format!("no sketch `{k}`"))),
        }
    }
    if ids.is_empty() {
        return Err(bad(cmd, "`sketches` must list sketches"));
    }
    for id in &ids {
        if let Some(f) = s.doc_mut().feature_mut(*id) {
            f.component = to;
        }
    }
    Ok(json!({"sketches": ids, "component": to}))
}

fn copy_occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.copy";
    let comp = if p.get("occurrence").is_some() {
        let o = occurrence(s, p, cmd)?;
        s.doc.occurrences.iter().find(|x| x.id == o).map(|x| x.component).unwrap_or(0)
    } else {
        component(s, p, "component", cmd)?
    };
    let src = s.doc.occurrence_of(comp).cloned().ok_or_else(|| bad(cmd, "that component has no occurrence"))?;
    let parent = if p.get("parent").is_some() { component(s, p, "parent", cmd)? } else { src.parent };
    let t = p.get("translate").and_then(vec3).unwrap_or(Vec3::ZERO);
    let m = solvecraft_doc::mat_mul(&translation(t), &src.transform);
    let id = s.doc_mut().add_occurrence(comp, parent, m)?;
    Ok(json!({"occurrence": id, "component": comp}))
}

fn paste_new(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.paste_new";
    let comp = component(s, p, "component", cmd)?;
    if comp == 0 {
        return Err(bad(cmd, "pick a component, not the design"));
    }
    let src = s.doc.components.iter().find(|c| c.id == comp).cloned().ok_or_else(|| bad(cmd, "component"))?;
    let occ = s.doc.occurrence_of(comp).cloned();
    let new_id = s.doc_mut().add_component(Some(&format!("{} (copy)", src.name)), src.parent)?;
    // Copy the component's features with fresh ids and names; references between them follow.
    let feats: Vec<solvecraft_doc::Feature> = s.doc.features.iter().filter(|f| f.component == comp).cloned().collect();
    let mut idmap: std::collections::HashMap<u64, u64> = Default::default();
    let mut namemap: std::collections::HashMap<String, String> = Default::default();
    let mut new_ids = Vec::new();
    for f in &feats {
        let nid = s.doc_mut().add_feature(f.kind.clone(), Some(&format!("{} (copy)", f.name)))?;
        idmap.insert(f.id, nid);
        let nname = s.doc.feature(nid).map(|x| x.name.clone()).unwrap_or_default();
        namemap.insert(f.name.clone(), nname);
        new_ids.push(nid);
    }
    for nid in &new_ids {
        let Some(f) = s.doc.feature(*nid).cloned() else { continue };
        let mut v = serde_json::to_value(&f.kind).map_err(|e| bad(cmd, e.to_string()))?;
        remap(&mut v, &idmap, &namemap, 0);
        let kind: FeatureKind = serde_json::from_value(v).map_err(|e| bad(cmd, e.to_string()))?;
        if let Some(slot) = s.doc_mut().feature_mut(*nid) {
            slot.kind = kind;
            slot.component = new_id;
        }
    }
    let t = p.get("translate").and_then(vec3).unwrap_or(Vec3::ZERO);
    let base = occ.map(|o| o.transform).unwrap_or(solvecraft_doc::IDENTITY);
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.component == new_id) {
        o.transform = solvecraft_doc::mat_mul(&translation(t), &base);
    }
    Ok(json!({"component": new_id, "features": new_ids}))
}

/// Point copied features at their copied sketches and features.
fn remap(v: &mut Value, ids: &std::collections::HashMap<u64, u64>, names: &std::collections::HashMap<String, String>, depth: usize) {
    if depth > 32 {
        return;
    }
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                match (k.as_str(), &*x) {
                    ("sketch" | "path_sketch", Value::Number(n)) => {
                        if let Some(m) = n.as_u64().and_then(|i| ids.get(&i)) {
                            *x = json!(m);
                        }
                    }
                    ("features", Value::Array(_)) => {
                        if let Value::Array(a) = x {
                            for e in a.iter_mut() {
                                if let Some(m) = e.as_str().and_then(|n| names.get(n)) {
                                    *e = json!(m);
                                }
                            }
                        }
                    }
                    _ => remap(x, ids, names, depth + 1),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| remap(x, ids, names, depth + 1)),
        _ => {}
    }
}

fn move_occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.move";
    let id = occurrence(s, p, cmd)?;
    let o = s.doc.occurrences.iter().find(|o| o.id == id).cloned().ok_or_else(|| bad(cmd, "occurrence"))?;
    if o.grounded {
        return Err(bad(cmd, format!("{} is grounded", o.name)));
    }
    let t = p.get("translate").and_then(vec3).unwrap_or(Vec3::ZERO);
    let axis = p.get("axis").and_then(vec3).unwrap_or(Vec3::Z);
    let angle = match p.get("angle") {
        Some(Value::String(e)) => s.doc.eval(e, solvecraft_doc::expr::Kind::Angle).map_err(|e| bad(cmd, e.to_string()))?,
        _ => num(p, "angle").unwrap_or(0.0),
    };
    if !(t.is_finite() && angle.is_finite()) {
        return Err(bad(cmd, "non-finite move"));
    }
    let origin = p.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
    let delta = solvecraft_doc::rigid(t, origin, axis, angle);
    let cur = s.pending_moves.get(&id).copied().unwrap_or(o.transform);
    let next = solvecraft_doc::mat_mul(&delta, &cur);
    if bool_(p, "capture").unwrap_or(true) {
        s.pending_moves.remove(&id);
        if let Some(x) = s.doc_mut().occurrences.iter_mut().find(|x| x.id == id) {
            x.transform = next;
        }
    } else {
        s.pending_moves.insert(id, next);
        s.revision += 1;
    }
    Ok(json!({"occurrence": id, "transform": next, "pending": !bool_(p, "capture").unwrap_or(true)}))
}

fn ground(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.ground";
    let id = occurrence(s, p, cmd)?;
    let want = bool_(p, "grounded");
    let o = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id).ok_or_else(|| bad(cmd, "occurrence"))?;
    o.grounded = want.unwrap_or(!o.grounded);
    let g = o.grounded;
    Ok(json!({"occurrence": id, "grounded": g}))
}

fn capture(s: &mut Session, _p: &Value) -> Result<Value> {
    let moves = std::mem::take(&mut s.pending_moves);
    let n = moves.len();
    for (id, m) in moves {
        if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
            o.transform = m;
        }
    }
    s.revision += 1;
    Ok(json!({ "captured": n }))
}

fn revert(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = s.pending_moves.len();
    s.pending_moves.clear();
    s.revision += 1;
    Ok(json!({ "reverted": n }))
}

/// The active component's frame from world coordinates (for new features' geometry).
pub fn to_active_frame(s: &Session, kind: &mut FeatureKind) {
    // Commands whose picks `frames` maps are already in their frames.
    if s.active_component == 0 || crate::frames::mapped() {
        return;
    }
    let m = s.doc.component_transform(s.active_component);
    if solvecraft_doc::is_identity(&m) {
        return;
    }
    if let Some(inv) = solvecraft_doc::mat_inverse(&m) {
        kind.transform_geometry(&inv);
    }
}
