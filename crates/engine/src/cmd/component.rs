//! Components: the design as a tree. New sketches and features go into the active component;
//! bodies belong to their feature's component unless moved. (Groundwork for assemblies:
//! occurrences, joints and per-component physical properties build on this.)

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{bad, str_, string_list};
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
];

fn component_key(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn new_component(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCreateNewComponentCommand";
    let parent = match component_key(p.get("parent")) {
        Some(k) => s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))?,
        None => s.active_component,
    };
    let id = s.doc_mut().add_component(str_(p, "name"), parent)?;
    if p.get("activate").and_then(Value::as_bool).unwrap_or(true) {
        s.active_component = id;
    }
    let name = s.doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    Ok(json!({"component": id, "name": name, "parent": parent}))
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
    let cmd = "component.activate";
    let k = component_key(p.get("component")).ok_or_else(|| bad(cmd, "`component` must be an id or name"))?;
    let id = s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))?;
    s.active_component = id;
    s.revision += 1;
    Ok(json!({ "active": id }))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.model.state();
    let doc = &s.doc;
    let node = |id: u64, name: &str, parent: Option<u64>| {
        let bodies: Vec<&str> = st.bodies.iter().filter(|b| doc.body_component(&b.name, b.feature) == id).map(|b| b.name.as_str()).collect();
        let features: Vec<&str> = doc.features.iter().filter(|f| f.component == id).map(|f| f.name.as_str()).collect();
        json!({"id": id, "name": name, "parent": parent, "bodies": bodies, "features": features})
    };
    let mut out = vec![node(0, &doc.name, None)];
    for c in &doc.components {
        out.push(node(c.id, &c.name, Some(c.parent)));
    }
    Ok(json!({"active": s.active_component, "components": out}))
}
