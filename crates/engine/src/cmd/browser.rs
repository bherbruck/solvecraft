//! Browser commands: renaming bodies, moving a sketch to another plane, and the browser's
//! groups and item order (kept in the design, so they are saved and undone like features).
//!
//! Browser items are keyed per component folder (`bodies`, `sketches`, `construction`): a body
//! by name, a sketch or construction plane by feature id.

use serde_json::{Value, json};

use solvecraft_doc::FeatureKind;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("body.rename", "Rename Body", rename_body).params("body: its name, name: the new name (features that use the body follow)"),
    CommandSpec::new("sketch.redefine", "Redefine Sketch Plane", redefine_sketch)
        .icon("plane")
        .params("sketch: id or name; plane: XY|XZ|YZ | construction plane name | {face: [x,y,z]} | {origin, normal} (the sketch keeps its geometry)"),
    CommandSpec::new("browser.group", "New Group", group)
        .icon("folder")
        .params("folder: bodies|sketches|construction; component?: id (default 0); name?; items?: [keys] (they leave other groups)"),
    CommandSpec::new("browser.ungroup", "Ungroup", ungroup).params("group: id (its items go back to the folder)"),
    CommandSpec::new("browser.rename_group", "Rename Group", rename_group).params("group: id, name"),
    CommandSpec::new("browser.move", "Move Browser Items", move_items)
        .params("folder, component?, items: [keys]; group?: id (into it; omit to take them out of groups); before?: key (position in the group)"),
    CommandSpec::new("document.units", "Change Design Units", document_units)
        .icon("settings")
        .params("units: mm|cm|m|in|ft (display and unit-less parameters; values keep their meaning)"),
    CommandSpec::new("view.save", "New Named View", view_save)
        .icon("camera")
        .params("name?; camera: {target: [x,y,z], yaw, pitch, distance, fov?} (replaces a view of the same name)"),
    CommandSpec::new("view.rename", "Rename Named View", view_rename).params("view: name, name"),
    CommandSpec::new("view.delete", "Delete Named View", view_delete).params("view: name"),
    CommandSpec::new("view.list", "List Named Views", view_list).noundo(),
    CommandSpec::new("browser.order", "Order Browser Items", order).params("folder, component?, order: [keys] (the folder's items in display order)"),
];

const FOLDERS: [&str; 3] = ["bodies", "sketches", "construction"];

fn folder_of(p: &Value, cmd: &str) -> Result<String> {
    match str_(p, "folder") {
        Some(f) if FOLDERS.contains(&f) => Ok(f.to_string()),
        _ => Err(bad(cmd, "`folder` must be bodies, sketches or construction")),
    }
}

fn component_of(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let c = p.get("component").and_then(Value::as_u64).unwrap_or(0);
    if c != 0 && !s.doc.components.iter().any(|x| x.id == c) {
        return Err(bad(cmd, format!("no component {c}")));
    }
    Ok(c)
}

fn keys(p: &Value, k: &str, cmd: &str) -> Result<Vec<String>> {
    let v = match p.get(k) {
        Some(Value::Array(a)) => {
            a.iter().map(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string()))).collect::<Option<Vec<_>>>()
        }
        None => Some(Vec::new()),
        _ => None,
    };
    let v = v.ok_or_else(|| bad(cmd, format!("`{k}` must be a list of item keys")))?;
    if v.len() > 10_000 || v.iter().any(|x| x.is_empty() || x.len() > 256) {
        return Err(bad(cmd, format!("`{k}` has bad keys")));
    }
    Ok(v)
}

fn group_id(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let id = p.get("group").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`group` must be a group id"))?;
    if s.doc.browser_groups.iter().any(|g| g.id == id) { Ok(id) } else { Err(bad(cmd, format!("no group {id}"))) }
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.group";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let items = keys(p, "items", cmd)?;
    let d = s.doc_mut();
    let id = d.browser_groups.iter().map(|g| g.id).max().unwrap_or(0) + 1;
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.len() <= 128 => n.to_string(),
        Some(_) => return Err(bad(cmd, "`name` is too long")),
        None => format!("Group{}", d.browser_groups.iter().filter(|g| g.folder == folder).count() + 1),
    };
    for g in d.browser_groups.iter_mut().filter(|g| g.component == component && g.folder == folder) {
        g.items.retain(|k| !items.contains(k));
    }
    d.browser_groups.push(solvecraft_doc::BrowserGroup { id, name: name.clone(), component, folder, items });
    Ok(json!({ "group": id, "name": name }))
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_id(s, p, "browser.ungroup")?;
    s.doc_mut().browser_groups.retain(|g| g.id != id);
    Ok(json!({ "group": id }))
}

fn rename_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.rename_group";
    let id = group_id(s, p, cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if let Some(g) = s.doc_mut().browser_groups.iter_mut().find(|g| g.id == id) {
        g.name = name.to_string();
    }
    Ok(json!({ "group": id, "name": name }))
}

fn move_items(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.move";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let items = keys(p, "items", cmd)?;
    if items.is_empty() {
        return Err(bad(cmd, "`items` must list item keys"));
    }
    let to = match p.get("group") {
        Some(Value::Null) | None => None,
        Some(_) => Some(group_id(s, p, cmd)?),
    };
    if let Some(g) = to
        && !s.doc.browser_groups.iter().any(|x| x.id == g && x.component == component && x.folder == folder)
    {
        return Err(bad(cmd, "that group is in another folder"));
    }
    let before = str_(p, "before").map(str::to_string);
    let d = s.doc_mut();
    for g in d.browser_groups.iter_mut().filter(|g| g.component == component && g.folder == folder) {
        g.items.retain(|k| !items.contains(k));
        if Some(g.id) == to {
            let at = before.as_ref().and_then(|b| g.items.iter().position(|k| k == b)).unwrap_or(g.items.len());
            for (i, k) in items.iter().enumerate() {
                g.items.insert(at + i, k.clone());
            }
        }
    }
    Ok(json!({ "items": items, "group": to }))
}

fn order(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.order";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let order = keys(p, "order", cmd)?;
    let key = format!("{component}/{folder}");
    let d = s.doc_mut();
    if order.is_empty() {
        d.browser_order.remove(&key);
    } else {
        d.browser_order.insert(key, order);
    }
    Ok(json!({}))
}

fn document_units(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "document.units";
    let u = str_(p, "units").ok_or_else(|| bad(cmd, "`units` is required"))?;
    if !["mm", "cm", "m", "in", "ft"].contains(&u) {
        return Err(bad(cmd, "`units` must be mm, cm, m, in or ft"));
    }
    let old = s.doc.units.clone();
    if old == u {
        return Ok(json!({ "units": u, "kept": [] }));
    }
    // What the geometry reads: every feature's length inputs.
    let before = s.doc.length_inputs();
    s.doc_mut().units = u.to_string();
    // Unit-less user parameters holding a bare number read it in the design's units where they
    // stand for lengths. If any input moved, those numbers keep the old unit, written out.
    let same = |a: &[Option<f64>]| {
        a.iter().zip(&before).filter(|(x, y)| matches!((x, y), (Some(x), Some(y)) if (x - y).abs() <= 1e-9 * y.abs().max(1.0))).count()
    };
    let after = s.doc.length_inputs();
    let mut kept = Vec::new();
    if same(&after) < before.len() {
        let bare: Vec<(String, String)> = s
            .doc
            .params
            .iter()
            .filter(|p| !p.model && p.unit.is_empty() && solvecraft_doc::expr::is_literal(&p.expr) && p.expr.trim().parse::<f64>().is_ok())
            .map(|p| (p.name.clone(), p.expr.trim().to_string()))
            .collect();
        for (name, e) in bare {
            let now = same(&s.doc.length_inputs());
            let probe = {
                let mut d = (*s.doc).clone();
                let _ = d.change_param(&name, &format!("{e} {old}"), Some(&old), None);
                same(&d.length_inputs())
            };
            // Only when writing the old unit puts more inputs back where they were.
            if probe > now {
                s.doc_mut().change_param(&name, &format!("{e} {old}"), Some(&old), None)?;
                kept.push(name);
            }
        }
    }
    Ok(json!({ "units": u, "kept": kept }))
}

fn view_save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.save";
    let c = p.get("camera").ok_or_else(|| bad(cmd, "`camera` is required"))?;
    let mut v: solvecraft_doc::NamedView = serde_json::from_value(c.clone()).map_err(|e| bad(cmd, format!("bad camera: {e}")))?;
    let ok = [v.target.x, v.target.y, v.target.z, v.yaw, v.pitch, v.distance, v.fov].iter().all(|x| x.is_finite()) && v.distance > 0.0;
    if !ok {
        return Err(bad(cmd, "the camera must be finite with a positive distance"));
    }
    let n = s.doc.named_views.len();
    v.name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.len() <= 128 => n.to_string(),
        Some(_) => return Err(bad(cmd, "`name` is too long")),
        None => (n + 1..).map(|k| format!("Named View{k}")).find(|x| !s.doc.named_views.iter().any(|v| v.name == *x)).unwrap_or_default(),
    };
    if n >= 1000 {
        return Err(bad(cmd, "too many named views"));
    }
    let name = v.name.clone();
    let d = s.doc_mut();
    d.named_views.retain(|x| x.name != name);
    d.named_views.push(v);
    Ok(json!({ "view": name }))
}

fn view_index(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let k = str_(p, "view").ok_or_else(|| bad(cmd, "`view` must be a view name"))?;
    s.doc.named_views.iter().position(|v| v.name == k).ok_or_else(|| bad(cmd, format!("no named view `{k}`")))
}

fn view_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.rename";
    let i = view_index(s, p, cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if s.doc.named_views.iter().enumerate().any(|(j, v)| j != i && v.name == name) {
        return Err(bad(cmd, format!("another view is called `{name}`")));
    }
    if let Some(v) = s.doc_mut().named_views.get_mut(i) {
        v.name = name.to_string();
    }
    Ok(json!({ "view": name }))
}

fn view_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let i = view_index(s, p, "view.delete")?;
    let v = s.doc_mut().named_views.remove(i);
    Ok(json!({ "deleted": v.name }))
}

fn view_list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({ "views": s.doc.named_views }))
}

/// Rename an item key in the browser's groups and orders of a folder.
fn rename_key(d: &mut solvecraft_doc::Document, folder: &str, old: &str, new: &str) {
    for g in d.browser_groups.iter_mut().filter(|g| g.folder == folder) {
        for k in g.items.iter_mut().filter(|k| *k == old) {
            *k = new.to_string();
        }
    }
    for (_, v) in d.browser_order.iter_mut().filter(|(k, _)| k.ends_with(&format!("/{folder}"))) {
        for k in v.iter_mut().filter(|k| *k == old) {
            *k = new.to_string();
        }
    }
}

/// Replace every string equal to `old` with `new`; true when something changed.
fn replace_str(v: &mut Value, old: &str, new: &str, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match v {
        Value::String(s) if s == old => {
            *s = new.to_string();
            true
        }
        Value::Array(a) => a.iter_mut().fold(false, |acc, x| replace_str(x, old, new, depth + 1) | acc),
        Value::Object(o) => o.values_mut().fold(false, |acc, x| replace_str(x, old, new, depth + 1) | acc),
        _ => false,
    }
}

fn rename_body(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "body.rename";
    let old = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must be a body name"))?.to_string();
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if name == old {
        return Ok(json!({"body": old, "name": name}));
    }
    let st = s.model.state();
    let b = st.body(&old).ok_or_else(|| bad(cmd, format!("no body `{old}`")))?;
    if st.body(name).is_some() {
        return Err(bad(cmd, format!("another body is called `{name}`")));
    }
    // The body is the k-th new body of its feature; the names before it are kept as they are.
    let fid = b.feature;
    let siblings: Vec<String> = st.bodies.iter().filter(|x| x.feature == fid).map(|x| x.name.clone()).collect();
    let k = siblings.iter().position(|n| *n == old).unwrap_or(0);
    let d = s.doc_mut();
    let f = d.feature_mut(fid).ok_or_else(|| bad(cmd, "the body's feature is gone"))?;
    while f.body_names.len() <= k {
        let i = f.body_names.len();
        f.body_names.push(siblings.get(i).cloned().unwrap_or_default());
    }
    if let Some(slot) = f.body_names.get_mut(k) {
        *slot = name.to_string();
    }
    // Later features refer to bodies by name.
    let mut failed = None;
    for f in d.features.iter_mut().filter(|f| f.id != fid) {
        let Ok(mut v) = serde_json::to_value(&f.kind) else { continue };
        if replace_str(&mut v, &old, name, 0) {
            match serde_json::from_value(v) {
                Ok(k) => f.kind = k,
                Err(e) => failed = Some(e.to_string()),
            }
        }
    }
    if let Some(e) = failed {
        return Err(bad(cmd, e));
    }
    if let Some(m) = d.materials.remove(&old) {
        d.materials.insert(name.to_string(), m);
    }
    if let Some(c) = d.appearances.bodies.remove(&old) {
        d.appearances.bodies.insert(name.to_string(), c);
    }
    for f in d.appearances.faces.iter_mut().filter(|f| f.body == old) {
        f.body = name.to_string();
    }
    if let Some(o) = d.body_offsets.remove(&old) {
        d.body_offsets.insert(name.to_string(), o);
    }
    if let Some(c) = d.body_components.remove(&old) {
        d.body_components.insert(name.to_string(), c);
    }
    rename_key(d, "bodies", &old, name);
    Ok(json!({"body": old, "name": name}))
}

fn redefine_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.redefine";
    let key = match p.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`sketch` must be an id or name")),
    };
    let id = match s.doc.find_feature(&key) {
        Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
    };
    if p.get("plane").is_none() {
        return Err(bad(cmd, "`plane` is required"));
    }
    let plane = super::sketch::plane_ref(s, p, cmd)?;
    let (vals, _) = s.doc.param_values();
    s.doc.resolve_plane(&vals, &plane, 0)?;
    if let Some(FeatureKind::Sketch { plane: slot, .. }) = s.doc_mut().feature_mut(id).map(|f| &mut f.kind) {
        *slot = plane;
    }
    Ok(json!({ "sketch": id }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    #[test]
    fn renamed_body_keeps_its_users() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        let [a, b] = [names(&s)[0].clone(), names(&s)[1].clone()];
        run(&mut s, "FusionMoveCommand", json!({"bodies": [b.clone()], "translate": [0, 0, 5]}));
        run(&mut s, "body.rename", json!({"body": b, "name": "Lid"}));
        assert_eq!(names(&s), vec![a.clone(), "Lid".to_string()]);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "the move still finds its body");
        // Duplicate names and unknown bodies are refused; undo restores the old name.
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": a})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Nope", "name": "X"})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": " "})).is_err());
        run(&mut s, "UndoCommand", json!({}));
        assert_eq!(names(&s)[1], b);
    }

    #[test]
    fn redefined_sketch_moves_its_extrude() {
        let mut s = Session::default();
        let sk = run(&mut s, "SketchCreate", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [10, 20]}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"distance": 5}));
        let zmax = |s: &mut Session| run(s, "MeasureCommand", json!({}))["bodies"][0]["bbox"]["max"][2].as_f64().unwrap();
        assert!((zmax(&mut s) - 5.0).abs() < 1e-6);
        run(&mut s, "sketch.redefine", json!({"sketch": sk, "plane": "XZ"}));
        let z = zmax(&mut s);
        assert!(z > 9.0, "the profile stands up on XZ: {z}");
        assert!(s.execute("sketch.redefine", &json!({"sketch": sk, "plane": "nope"})).is_err());
        assert!(s.execute("sketch.redefine", &json!({"sketch": 999, "plane": "XY"})).is_err());
    }

    #[test]
    fn design_units_change_without_moving_geometry() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 20, "width": 10, "height": 5}));
        let vol = |s: &mut Session| run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        let v0 = vol(&mut s);
        run(&mut s, "document.units", json!({"units": "in"}));
        assert_eq!(s.doc.units, "in");
        assert!((vol(&mut s) - v0).abs() < 1e-6, "feature inputs keep their millimetres");
        assert!(s.execute("document.units", &json!({"units": "furlong"})).is_err());
        // A unit-less parameter used as a length keeps its millimetres; a plain count stays a number.
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "w", "expression": "5", "unit": ""}));
        run(&mut s, "parameters.add", json!({"name": "n", "expression": "3", "unit": ""}));
        run(&mut s, "PrimitiveBox", json!({"length": "w", "width": 10, "height": 5}));
        let b = s.model.state().bodies[0].name.clone();
        run(&mut s, "PatternRectangular", json!({"bodies": [b], "dir1": [1, 0, 0], "count1": "n", "spacing1": 20}));
        let v0 = vol(&mut s);
        let r = run(&mut s, "document.units", json!({"units": "cm"}));
        assert!((vol(&mut s) - v0).abs() < 1e-6, "{r}");
        assert!(s.doc.param("n").is_some_and(|p| p.expr == "3"), "the count stays bare");
        run(&mut s, "UndoCommand", json!({}));
        assert_eq!(s.doc.units, "mm");
    }

    #[test]
    fn named_views_are_saved_renamed_and_deleted() {
        let mut s = Session::default();
        let cam = json!({"target": [1, 2, 3], "yaw": 0.5, "pitch": 0.3, "distance": 100});
        assert_eq!(run(&mut s, "view.save", json!({"camera": cam}))["view"], "Named View1");
        run(&mut s, "view.save", json!({"camera": cam, "name": "Detail"}));
        run(&mut s, "view.rename", json!({"view": "Named View1", "name": "Overview"}));
        assert!(s.execute("view.rename", &json!({"view": "Overview", "name": "Detail"})).is_err());
        let back: solvecraft_doc::Document = serde_json::from_str(&serde_json::to_string(&*s.doc).unwrap()).unwrap();
        assert_eq!(back.named_views.len(), 2);
        assert_eq!(back.named_views[0].target.z, 3.0);
        run(&mut s, "view.delete", json!({"view": "Detail"}));
        assert_eq!(run(&mut s, "view.list", json!({}))["views"].as_array().unwrap().len(), 1);
        for p in [json!({}), json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": -1}}), json!({"camera": "x"})] {
            assert!(s.execute("view.save", &p).is_err(), "{p}");
        }
        run(&mut s, "UndoCommand", json!({}));
        assert_eq!(s.doc.named_views.len(), 2);
    }

    #[test]
    fn groups_hold_items_and_follow_renames() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        let [a, b] = [names(&s)[0].clone(), names(&s)[1].clone()];
        let g = run(&mut s, "browser.group", json!({"folder": "bodies", "items": [a.clone()]}))["group"].as_u64().unwrap();
        assert_eq!(s.doc.browser_groups[0].name, "Group1");
        run(&mut s, "browser.move", json!({"folder": "bodies", "items": [b.clone()], "group": g, "before": a.clone()}));
        assert_eq!(s.doc.browser_groups[0].items, vec![b.clone(), a.clone()]);
        run(&mut s, "browser.rename_group", json!({"group": g, "name": "Fasteners"}));
        run(&mut s, "body.rename", json!({"body": a, "name": "Bolt"}));
        assert_eq!(s.doc.browser_groups[0].items, vec![b.clone(), "Bolt".to_string()]);
        // Out of the group, then a folder order; saved with the design.
        run(&mut s, "browser.move", json!({"folder": "bodies", "items": [b.clone()]}));
        assert_eq!(s.doc.browser_groups[0].items, vec!["Bolt".to_string()]);
        run(&mut s, "browser.order", json!({"folder": "bodies", "order": [b.clone(), "Bolt"]}));
        let saved: solvecraft_doc::Document = serde_json::from_str(&serde_json::to_string(&*s.doc).unwrap()).unwrap();
        assert_eq!(saved.browser_groups, s.doc.browser_groups);
        assert_eq!(saved.browser_order["0/bodies"], vec![b, "Bolt".to_string()]);
        run(&mut s, "browser.ungroup", json!({"group": g}));
        assert!(s.doc.browser_groups.is_empty());
        // Hostile input is refused.
        for (c, p) in [
            ("browser.group", json!({"folder": "nope"})),
            ("browser.group", json!({"folder": "bodies", "component": 99})),
            ("browser.group", json!({"folder": "bodies", "items": "x"})),
            ("browser.move", json!({"folder": "bodies", "items": []})),
            ("browser.move", json!({"folder": "bodies", "items": ["x"], "group": 42})),
            ("browser.ungroup", json!({"group": -1})),
            ("browser.rename_group", json!({"group": g, "name": "x"})),
        ] {
            assert!(s.execute(c, &p).is_err(), "{c} {p}");
        }
        run(&mut s, "UndoCommand", json!({}));
        assert_eq!(s.doc.browser_groups.len(), 1, "ungroup undoes");
    }
}
