//! Undo/redo, parameters, the timeline, deletion and selection.

use serde_json::{Value, json};
use solvecraft_doc::{Feature, FeatureKind};

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, string_list};
use crate::{EngineError, Result, Sel, Session, Snapshot};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("UndoCommand", "Undo", undo).icon("undo").key("Ctrl+Z").noundo(),
    CommandSpec::new("RedoCommand", "Redo", redo).icon("redo").key("Ctrl+Y").noundo(),
    CommandSpec::new("ChangeParameterCommand", "Change Parameters", change_param)
        .at("SOLID", "MODIFY")
        .icon("params")
        .params("name, expression (number or text), unit?: mm|deg|\"\", comment?; or delete: name"),
    CommandSpec::new("FusionComputeAllCommand", "Compute All", compute_all).at("SOLID", "MODIFY").icon("compute").noundo(),
    CommandSpec::new("FusionDeleteCommand", "Delete", delete)
        .at("SOLID", "MODIFY")
        .icon("delete")
        .key("Delete")
        .params("features: [id or name] (a sketch takes its dependent features with it)"),
    CommandSpec::new("FusionRenameTimelineEntryCommand", "Rename", rename).params("feature: id|name, name"),
    CommandSpec::new("timeline.rollback", "Move Timeline Marker", rollback).params("position: number of features to keep active (omit = end)"),
    CommandSpec::new("timeline.suppress", "Suppress Feature", suppress).params("feature: id|name, suppressed?: bool (default toggles)"),
    CommandSpec::new("timeline.edit", "Edit Feature", edit_feature)
        .params("feature: id|name, set: {fields to change, e.g. {\"extent\": {\"distance\": \"30\"}}}"),
    CommandSpec::new("select.set", "Select", select_set)
        .noundo()
        .params("items: [{type: body|edge|face|feature|sketch_curve|sketch_point|profile, …}], add?: bool"),
    CommandSpec::new("select.clear", "Clear Selection", select_clear).noundo(),
];

fn undo(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(prev) = s.undo.pop() else { return Err(EngineError::Other("nothing to undo".into())) };
    let label = prev.label.clone();
    s.redo.push(Snapshot { label: label.clone(), doc: s.doc.clone(), active_sketch: s.active_sketch });
    s.doc = prev.doc;
    s.active_sketch = prev.active_sketch;
    s.selection.clear();
    s.refresh();
    Ok(json!({"undone": label}))
}

fn redo(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(next) = s.redo.pop() else { return Err(EngineError::Other("nothing to redo".into())) };
    let label = next.label.clone();
    s.undo.push(Snapshot { label: label.clone(), doc: s.doc.clone(), active_sketch: s.active_sketch });
    s.doc = next.doc;
    s.active_sketch = next.active_sketch;
    s.selection.clear();
    s.refresh();
    Ok(json!({"redone": label}))
}

fn change_param(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ChangeParameterCommand";
    if let Some(n) = str_(p, "delete") {
        s.doc_mut().remove_param(n)?;
        return Ok(json!({"deleted": n}));
    }
    let name = str_(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?;
    let e = match p.get("expression").or_else(|| p.get("value")) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(bad(cmd, "`expression` must be a number or text")),
    };
    let unit = str_(p, "unit");
    let comment = str_(p, "comment");
    s.doc_mut().set_param(name, &e, unit, comment)?;
    s.refresh();
    let (vals, _) = s.doc.param_values();
    let v = vals.get(name).map(|v| v.v);
    // Report features that now fail.
    let errors: Vec<Value> = s.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| json!({"feature": r.name, "error": e}))).collect();
    Ok(json!({"name": name, "expression": e, "value": v, "recomputed": s.model.last_recomputed, "errors": errors}))
}

fn compute_all(s: &mut Session, _p: &Value) -> Result<Value> {
    s.model = solvecraft_doc::Model::new();
    s.refresh();
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    Ok(json!({"features": s.model.results.len(), "errors": errors}))
}

fn feature_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`feature` must be an id or name")),
    };
    s.doc.find_feature(&key).map(|f| f.id).ok_or_else(|| bad(cmd, format!("no feature `{key}`")))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionDeleteCommand";
    let mut keys = string_list(p, "features");
    if let Some(Value::Array(a)) = p.get("features") {
        keys.extend(a.iter().filter_map(Value::as_u64).map(|n| n.to_string()));
    }
    if let Some(f) = p.get("feature") {
        keys.push(match f {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => String::new(),
        });
    }
    if keys.is_empty() {
        return Err(bad(cmd, "`features` must list features"));
    }
    let mut gone = Vec::new();
    for k in keys {
        let Some(id) = s.doc.find_feature(&k).map(|f| f.id) else {
            if gone.is_empty() {
                return Err(bad(cmd, format!("no feature `{k}`")));
            }
            continue;
        };
        gone.extend(s.doc_mut().delete_feature(id)?);
    }
    if s.active_sketch.is_some_and(|a| gone.contains(&a)) {
        s.active_sketch = None;
    }
    s.selection.clear();
    Ok(json!({"deleted": gone}))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionRenameTimelineEntryCommand";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if s.doc.features.iter().any(|f| f.name == name && f.id != id) {
        return Err(bad(cmd, format!("another feature is called `{name}`")));
    }
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.name = name.to_string();
    }
    Ok(json!({"feature": id, "name": name}))
}

fn rollback(s: &mut Session, p: &Value) -> Result<Value> {
    let n = s.doc.features.len();
    let pos = num(p, "position").map(|x| x.max(0.0) as usize);
    s.doc_mut().marker = match pos {
        Some(k) if k < n => Some(k),
        _ => None,
    };
    s.selection.clear();
    Ok(json!({"marker": s.doc.marker.unwrap_or(n), "features": n}))
}

fn suppress(s: &mut Session, p: &Value) -> Result<Value> {
    let id = feature_id(s, p.get("feature"), "timeline.suppress")?;
    let want = bool_(p, "suppressed");
    let f = s.doc_mut().feature_mut(id).ok_or_else(|| EngineError::Other("feature".into()))?;
    f.suppressed = want.unwrap_or(!f.suppressed);
    Ok(json!({"feature": id, "suppressed": f.suppressed}))
}

fn merge(dst: &mut Value, src: &Value, depth: usize) {
    if depth > 16 {
        return;
    }
    match (dst, src) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                match d.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v, depth + 1),
                    _ => {
                        d.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (d, s) => *d = s.clone(),
    }
}

fn edit_feature(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.edit";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let set = p.get("set").filter(|v| v.is_object()).ok_or_else(|| bad(cmd, "`set` must be an object of fields"))?;
    let f = s.doc.feature(id).cloned().ok_or_else(|| bad(cmd, "feature"))?;
    let mut v = serde_json::to_value(&f).map_err(|e| EngineError::Other(e.to_string()))?;
    merge(&mut v, set, 0);
    let mut nf: Feature = serde_json::from_value(v).map_err(|e| bad(cmd, format!("invalid feature: {e}")))?;
    nf.id = f.id;
    if std::mem::discriminant(&nf.kind) != std::mem::discriminant(&f.kind) {
        return Err(bad(cmd, "the feature type cannot change"));
    }
    if let (FeatureKind::Sketch { sketch: a, .. }, FeatureKind::Sketch { sketch: b, .. }) = (&nf.kind, &f.kind)
        && a != b
    {
        return Err(bad(cmd, "edit sketch geometry with the sketch commands"));
    }
    if let Some(slot) = s.doc_mut().feature_mut(id) {
        *slot = nf;
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    Ok(json!({"feature": id, "recomputed": s.model.last_recomputed}))
}

fn select_set(s: &mut Session, p: &Value) -> Result<Value> {
    let items = p.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    if items.len() > 10_000 {
        return Err(bad("select.set", "too many items"));
    }
    let mut sel: Vec<Sel> = Vec::new();
    for it in items {
        let one: Sel = serde_json::from_value(it).map_err(|e| bad("select.set", e.to_string()))?;
        sel.push(one);
    }
    if !bool_(p, "add").unwrap_or(false) {
        s.selection.clear();
    }
    for x in sel {
        if !s.selection.contains(&x) {
            s.selection.push(x);
        }
    }
    s.revision += 1;
    Ok(json!({"selected": s.selection.len()}))
}

fn select_clear(s: &mut Session, _p: &Value) -> Result<Value> {
    s.selection.clear();
    s.revision += 1;
    Ok(json!({"selected": 0}))
}
