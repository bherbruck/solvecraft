//! Joints between occurrences (`solvecraft_doc::joints`): Joint Origin, Joint, As-built Joint,
//! Rigid Group, Drive Joints, Motion Link, limits, and Interference. Every change re-solves the
//! joint graph and moves the occurrences it places; the session re-solves after any edit too,
//! so joints follow the geometry their origins snap to.

use serde_json::{Value, json};
use solvecraft_doc::expr::Kind;
use solvecraft_doc::joints::{self, Joint, JointKind, JointOrigin, MotionLink, NamedOrigin, Snap};
use solvecraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, string_list, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("JointOrigin", "Joint Origin", joint_origin).at("SOLID", "CREATE").icon("joint_origin").params(
        "name?, occurrence?: id|name (default: from the pick), face: [x,y,z] (centre of a planar face) | circle: [x,y,z] (centre of a round end) | point: [x,y,z], z?: [x,y,z]",
    ),
    CommandSpec::new("JointAssembleCmdNew", "Joint", joint).at("SOLID", "ASSEMBLE").icon("joint").key("J").params(
        "type: rigid|revolute|slider|cylindrical|pin_slot|planar|ball; a, b: joint origins ({occurrence, face|circle|point: [x,y,z] in world} | origin name); \
         values?: [angles in deg or expressions, distances]; limits?: [[min, max] | null…]; flip?: bool; offset?; angle?; name?",
    ),
    CommandSpec::new("JointAsBuiltCmd", "As-Built Joint", as_built).at("SOLID", "ASSEMBLE").icon("as_built").params(
        "type, a: occurrence, b: occurrence (stay where they are); at?: joint origin (default: b's origin) — the joint's frame",
    ),
    CommandSpec::new("RigidGroupCmd", "Rigid Group", rigid_group).at("SOLID", "ASSEMBLE").icon("rigid_group").params("occurrences: [ids or names] (move together, as they are)"),
    CommandSpec::new("FusionMoveJointsCommand", "Drive Joints", drive).at("SOLID", "ASSEMBLE").icon("drive").params(
        "joint: id|name; value | values: [angles in deg or expressions, distances] (clamped to the limits)",
    ),
    CommandSpec::new("FusionMotionRelationshipCommand", "Motion Link", motion_link)
        .at("SOLID", "ASSEMBLE")
        .icon("motion_link")
        .params("a, b: joints; ratio (b = ratio · a + offset); offset?; ia?, ib?: value indices (default 0)"),
    CommandSpec::new("joint.limits", "Joint Limits", limits).params("joint; index?: value index (default 0); min?, max? (omit both to clear)"),
    CommandSpec::new("joint.edit", "Edit Joint", edit).params("joint; type?, flip?, offset?, angle?, suppressed?, name?"),
    CommandSpec::new("joint.delete", "Delete Joint", delete).params("joint: id|name (or origin: name, link: index)"),
    CommandSpec::new("joint.list", "List Joints", list).noundo().params("→ joints (type, occurrences, values, limits), origins, links, degrees of freedom, conflicts"),
    CommandSpec::new("joint.solve", "Solve Joints", solve_cmd).params("re-place the occurrences from the joints → conflicts, errors, dof"),
    CommandSpec::new("InterferenceCheckCommand", "Interference", interference).at("SOLID", "INSPECT").icon("interference").noundo().params(
        "bodies?: [names] (default: all, in world placement) → pairs that overlap with the overlap volume, area, bounding box and centre",
    ),
];

/// Re-solve the joints and move the occurrences they place (after any edit).
pub fn resolve(s: &mut Session) -> joints::Solution {
    let st = s.model.state();
    let sol = joints::solve(&s.doc, &st);
    if !sol.world.is_empty() {
        let mut next = (*s.doc).clone();
        if next.apply_joint_solution(&sol) {
            *s.doc_mut() = next;
            s.revision += 1;
        }
    }
    sol
}

fn sol_json(sol: &joints::Solution) -> Value {
    json!({
        "conflicts": sol.conflicts.iter().map(|(id, n, w)| json!({"joint": id, "name": n, "why": w})).collect::<Vec<_>>(),
        "errors": sol.errors.iter().map(|(id, n, w)| json!({"joint": id, "name": n, "why": w})).collect::<Vec<_>>(),
        "dof": sol.dof.iter().map(|(o, d)| json!({"occurrence": o, "dof": d})).collect::<Vec<_>>(),
        "fixed": sol.fixed,
    })
}

pub(crate) fn occurrence_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "an occurrence must be an id or a name")),
    };
    if key == "0" || key.eq_ignore_ascii_case("root") {
        return Ok(0);
    }
    if let Some(o) = s.doc.occurrences.iter().find(|o| o.id.to_string() == key || o.name == key) {
        return Ok(o.id);
    }
    // A component name: its first occurrence.
    s.doc.find_component(&key).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).ok_or_else(|| bad(cmd, format!("no occurrence `{key}`")))
}

/// The occurrence whose bodies are nearest a world point (0 = root bodies).
fn occurrence_at(s: &Session, p: Vec3) -> Option<u64> {
    let st = s.model.state();
    let mut best: Option<(f64, u64)> = None;
    for b in &st.bodies {
        let comp = s.doc.body_component(&b.name, b.feature);
        let occs: Vec<u64> = if comp == 0 { vec![0] } else { s.doc.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect() };
        for occ in occs {
            let t = if occ == 0 { solvecraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occ) };
            let inv = solvecraft_doc::mat_inverse(&t)?;
            let q = solvecraft_doc::apply_point(&inv, p);
            let bb = b.mesh().bounds();
            let c = Vec3::new(q.x.clamp(bb.min.x, bb.max.x), q.y.clamp(bb.min.y, bb.max.y), q.z.clamp(bb.min.z, bb.max.z));
            let d = c.dist(q);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, occ));
            }
        }
    }
    best.map(|(_, o)| o)
}

/// A joint origin from parameters: a named origin, or a snap given in world coordinates.
fn origin_param(s: &Session, v: Option<&Value>, cmd: &str) -> Result<JointOrigin> {
    let v = v.ok_or_else(|| bad(cmd, "joint origins `a` and `b` are required"))?;
    if let Value::String(name) = v {
        return s
            .doc
            .assembly
            .origins
            .iter()
            .find(|o| &o.name == name)
            .map(|o| o.origin.clone())
            .ok_or_else(|| bad(cmd, format!("no joint origin `{name}`")));
    }
    let pick = ["face", "circle", "point"].iter().find_map(|k| v.get(*k).and_then(vec3).map(|p| (*k, p)));
    let (what, world) = match (pick, v.get("frame")) {
        (Some(x), _) => x,
        (None, Some(f)) => ("frame", f.get("origin").and_then(vec3).ok_or_else(|| bad(cmd, "`frame` needs origin and z"))?),
        _ => return Err(bad(cmd, "a joint origin needs face, circle, point or frame [x, y, z]")),
    };
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None => occurrence_at(s, world).ok_or_else(|| bad(cmd, "no body there"))?,
    };
    // World → the component's own coordinates.
    let t = if occurrence == 0 { solvecraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solvecraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let local = |p: Vec3| solvecraft_doc::apply_point(&inv, p);
    let local_dir = |d: Vec3| solvecraft_doc::apply_vector(&inv, d);
    let snap = match what {
        "face" => Snap::FaceCenter { pick: local(world) },
        "circle" => Snap::CircleCenter { pick: local(world) },
        "point" => Snap::Point { point: local(world), z: v.get("z").and_then(vec3).map(local_dir) },
        _ => {
            let f = v.get("frame").cloned().unwrap_or_default();
            let z = f.get("z").and_then(vec3).ok_or_else(|| bad(cmd, "`frame` needs a z axis"))?;
            Snap::Frame { origin: local(world), z: local_dir(z), x: f.get("x").and_then(vec3).map(local_dir) }
        }
    };
    let o = JointOrigin { occurrence, snap };
    joints::resolve_origin(&s.doc, &s.model.state(), &o).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(o)
}

pub(crate) fn joint_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`joint` must be an id or a name")),
    };
    s.doc.assembly.joints.iter().find(|j| j.id.to_string() == key || j.name == key).map(|j| j.id).ok_or_else(|| bad(cmd, format!("no joint `{key}`")))
}

/// A joint value: numbers are degrees for angles and mm for distances; text is an expression.
pub(crate) fn value(s: &Session, v: &Value, angle: bool, cmd: &str) -> Result<f64> {
    let kind = if angle { Kind::Angle } else { Kind::Length };
    let e = match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite() && x.abs() < 1e9).map(|x| x.to_string()),
        Value::String(t) => Some(t.clone()),
        _ => None,
    }
    .ok_or_else(|| bad(cmd, "values must be numbers or expressions"))?;
    s.doc.eval(&e, kind).map_err(|e| bad(cmd, e.to_string()))
}

fn values_param(s: &Session, p: &Value, kind: JointKind, cmd: &str) -> Result<Vec<f64>> {
    let n = kind.dofs().len();
    let list: Vec<Value> = match (p.get("values"), p.get("value")) {
        (Some(Value::Array(a)), _) => a.clone(),
        (_, Some(v)) => vec![v.clone()],
        _ => Vec::new(),
    };
    if list.len() > n {
        return Err(bad(cmd, format!("a {kind:?} joint has {n} values ({})", kind.dofs().join(", "))));
    }
    let mut out = vec![0.0; n];
    for (i, v) in list.iter().enumerate() {
        if let Some(slot) = out.get_mut(i) {
            *slot = value(s, v, kind.is_angle(i), cmd)?;
        }
    }
    Ok(out)
}

fn limits_param(s: &Session, p: &Value, kind: JointKind, cmd: &str) -> Result<Vec<Option<(f64, f64)>>> {
    let n = kind.dofs().len();
    let mut out = vec![None; n];
    if let Some(Value::Array(a)) = p.get("limits") {
        if a.len() > n {
            return Err(bad(cmd, "more limits than values"));
        }
        for (i, l) in a.iter().enumerate() {
            if let (Some(lo), Some(hi)) = (l.get(0), l.get(1))
                && let Some(slot) = out.get_mut(i)
            {
                *slot = Some((value(s, lo, kind.is_angle(i), cmd)?, value(s, hi, kind.is_angle(i), cmd)?));
            }
        }
    }
    Ok(out)
}

fn next_joint_id(s: &Session) -> u64 {
    s.doc.assembly.joints.iter().map(|j| j.id).max().unwrap_or(0) + 1
}

fn add_joint(s: &mut Session, j: Joint) -> Result<Value> {
    if s.doc.assembly.joints.len() >= 10_000 {
        return Err(EngineError::Other("too many joints".into()));
    }
    if j.a.occurrence == j.b.occurrence {
        return Err(bad("JointAssembleCmdNew", "a joint needs two different occurrences"));
    }
    let id = j.id;
    let name = j.name.clone();
    s.doc_mut().assembly.joints.push(j);
    let sol = resolve(s);
    let mut out = sol_json(&sol);
    out["joint"] = json!(id);
    out["name"] = json!(name);
    Ok(out)
}

fn joint_origin(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "JointOrigin";
    let origin = origin_param(s, Some(p), cmd)?;
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => (1..).map(|k| format!("Joint Origin{k}")).find(|n| !s.doc.assembly.origins.iter().any(|o| &o.name == n)).unwrap_or_default(),
    };
    if s.doc.assembly.origins.iter().any(|o| o.name == name) {
        return Err(bad(cmd, format!("there is already a joint origin `{name}`")));
    }
    s.doc_mut().assembly.origins.push(NamedOrigin { name: name.clone(), origin });
    Ok(json!({"origin": name}))
}

fn kind_param(p: &Value, cmd: &str) -> Result<JointKind> {
    str_(p, "type")
        .and_then(JointKind::parse)
        .ok_or_else(|| bad(cmd, "`type` must be rigid, revolute, slider, cylindrical, pin_slot, planar or ball"))
}

fn joint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "JointAssembleCmdNew";
    let kind = kind_param(p, cmd)?;
    let a = origin_param(s, p.get("a"), cmd)?;
    let b = origin_param(s, p.get("b"), cmd)?;
    let id = next_joint_id(s);
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| format!("Joint{id}"));
    let offset = match p.get("offset") {
        Some(v) => value(s, v, false, cmd)?,
        None => 0.0,
    };
    let angle = match p.get("angle") {
        Some(v) => value(s, v, true, cmd)?,
        None => 0.0,
    };
    let j = Joint {
        id,
        name,
        kind,
        a,
        b,
        values: values_param(s, p, kind, cmd)?,
        limits: limits_param(s, p, kind, cmd)?,
        flip: bool_(p, "flip").unwrap_or(false),
        offset,
        angle,
        suppressed: false,
    };
    add_joint(s, j)
}

fn as_built(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "JointAsBuiltCmd";
    let kind = kind_param(p, cmd)?;
    let (oa, ob) = (occurrence_id(s, p.get("a"), cmd)?, occurrence_id(s, p.get("b"), cmd)?);
    // The joint's frame in the world: the given origin, or B's component origin.
    let w = match p.get("at") {
        Some(v) => {
            let o = origin_param(s, Some(v), cmd)?;
            s.doc.origin_world(&s.model.state(), &o).map_err(|e| bad(cmd, e.to_string()))?
        }
        None => joints::occurrence_world(&s.doc, ob),
    };
    let fa = s.doc.frame_in_occurrence(oa, &w).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let fb = s.doc.frame_in_occurrence(ob, &w).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let id = next_joint_id(s);
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| format!("Joint{id}"));
    let n = kind.dofs().len();
    let j = Joint {
        id,
        name,
        kind,
        a: JointOrigin { occurrence: oa, snap: fa },
        b: JointOrigin { occurrence: ob, snap: fb },
        values: vec![0.0; n],
        limits: limits_param(s, p, kind, cmd)?,
        flip: false,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
    };
    add_joint(s, j)
}

fn rigid_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "RigidGroupCmd";
    let list = p.get("occurrences").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.len() < 2 || list.len() > 1000 {
        return Err(bad(cmd, "`occurrences` must list 2…1000 occurrences"));
    }
    let ids: Vec<u64> = list.iter().map(|v| occurrence_id(s, Some(v), cmd)).collect::<Result<_>>()?;
    let first = ids.first().copied().unwrap_or(0);
    let mut made = Vec::new();
    for other in ids.iter().skip(1) {
        let r = as_built(s, &json!({"type": "rigid", "a": first, "b": other, "name": format!("Rigid Group {first}:{other}")}))?;
        made.push(r["joint"].clone());
    }
    Ok(json!({"joints": made}))
}

fn drive(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionMoveJointsCommand";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let kind = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind).ok_or_else(|| bad(cmd, "joint"))?;
    if kind == JointKind::Rigid {
        return Err(bad(cmd, "a rigid joint does not move"));
    }
    let mut q = values_param(s, p, kind, cmd)?;
    if p.get("values").is_none() && p.get("value").is_none() {
        return Err(bad(cmd, "give `value` or `values`"));
    }
    let doc = s.doc_mut();
    let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
    // Values not given keep theirs.
    if let Some(Value::Array(a)) = p.get("values") {
        for (i, v) in q.iter_mut().enumerate().skip(a.len()) {
            *v = j.values.get(i).copied().unwrap_or(*v);
        }
    }
    joints::clamp(j, &mut q);
    // Moved as far toward `q` as contact sets allow.
    let r = super::motion::drive_joint(s, id, q)?;
    let sol = joints::solve(&s.doc, &s.model.state());
    let mut out = sol_json(&sol);
    out["joint"] = json!(id);
    out["values"] = json!(s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.values.clone()));
    out["stopped_by_contact"] = r["stopped_by_contact"].clone();
    Ok(out)
}

fn motion_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionMotionRelationshipCommand";
    let (a, b) = (joint_id(s, p.get("a"), cmd)?, joint_id(s, p.get("b"), cmd)?);
    if a == b {
        return Err(bad(cmd, "link two different joints"));
    }
    let ratio = num(p, "ratio").ok_or_else(|| bad(cmd, "`ratio` must be a number"))?;
    let (ia, ib) = (num(p, "ia").unwrap_or(0.0).max(0.0) as usize, num(p, "ib").unwrap_or(0.0).max(0.0) as usize);
    let dofs = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind.dofs().len()).unwrap_or(0);
    if ia >= dofs(a) || ib >= dofs(b) {
        return Err(bad(cmd, "the linked joints need moving values at those indices"));
    }
    let offset = num(p, "offset").unwrap_or(0.0);
    s.doc_mut().assembly.links.push(MotionLink { a, ia, b, ib, ratio, offset });
    let sol = resolve(s);
    Ok(sol_json(&sol))
}

fn limits(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.limits";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let i = num(p, "index").unwrap_or(0.0).max(0.0) as usize;
    let kind = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind).ok_or_else(|| bad(cmd, "joint"))?;
    if i >= kind.dofs().len() {
        return Err(bad(cmd, "no such value"));
    }
    let lim = match (p.get("min"), p.get("max")) {
        (Some(lo), Some(hi)) => Some((value(s, lo, kind.is_angle(i), cmd)?, value(s, hi, kind.is_angle(i), cmd)?)),
        (None, None) => None,
        _ => return Err(bad(cmd, "give both `min` and `max`, or neither to clear")),
    };
    let doc = s.doc_mut();
    let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
    j.limits.resize(kind.dofs().len(), None);
    if let Some(slot) = j.limits.get_mut(i) {
        *slot = lim;
    }
    let mut q = j.values.clone();
    q.resize(kind.dofs().len(), 0.0);
    joints::clamp(j, &mut q);
    j.values = q;
    let sol = resolve(s);
    Ok(sol_json(&sol))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.edit";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let kind = match str_(p, "type") {
        Some(t) => Some(JointKind::parse(t).ok_or_else(|| bad(cmd, "unknown joint type"))?),
        None => None,
    };
    let offset = match p.get("offset") {
        Some(v) => Some(value(s, v, false, cmd)?),
        None => None,
    };
    let angle = match p.get("angle") {
        Some(v) => Some(value(s, v, true, cmd)?),
        None => None,
    };
    let doc = s.doc_mut();
    let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
    if let Some(k) = kind {
        j.kind = k;
        j.values.resize(k.dofs().len(), 0.0);
        j.limits.resize(k.dofs().len(), None);
    }
    if let Some(o) = offset {
        j.offset = o;
    }
    if let Some(a) = angle {
        j.angle = a;
    }
    if let Some(f) = bool_(p, "flip") {
        j.flip = f;
    }
    if let Some(x) = bool_(p, "suppressed") {
        j.suppressed = x;
    }
    if let Some(n) = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128) {
        j.name = n.to_string();
    }
    let sol = resolve(s);
    Ok(sol_json(&sol))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.delete";
    if let Some(name) = str_(p, "origin") {
        let used = s.doc.assembly.joints.len();
        let doc = s.doc_mut();
        let before = doc.assembly.origins.len();
        doc.assembly.origins.retain(|o| o.name != name);
        if doc.assembly.origins.len() == before {
            return Err(bad(cmd, format!("no joint origin `{name}`")));
        }
        return Ok(json!({"deleted_origin": name, "joints": used}));
    }
    if let Some(i) = num(p, "link") {
        let i = i.max(0.0) as usize;
        let doc = s.doc_mut();
        if i >= doc.assembly.links.len() {
            return Err(bad(cmd, "no such motion link"));
        }
        doc.assembly.links.remove(i);
        return Ok(json!({"deleted_link": i}));
    }
    let id = joint_id(s, p.get("joint"), cmd)?;
    let doc = s.doc_mut();
    doc.assembly.joints.retain(|j| j.id != id);
    doc.assembly.links.retain(|l| l.a != id && l.b != id);
    let sol = resolve(s);
    let mut out = sol_json(&sol);
    out["deleted"] = json!(id);
    Ok(out)
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.model.state();
    let sol = joints::solve(&s.doc, &st);
    let vals = joints::joint_values(&s.doc.assembly);
    let js: Vec<Value> = s
        .doc
        .assembly
        .joints
        .iter()
        .map(|j| {
            let q = vals.get(&j.id).cloned().unwrap_or_default();
            let named: Vec<Value> = j
                .kind
                .dofs()
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let v = q.get(i).copied().unwrap_or(0.0);
                    json!({"name": n, "value": if j.kind.is_angle(i) { v.to_degrees() } else { v }, "unit": if j.kind.is_angle(i) { "deg" } else { "mm" },
                        "limits": j.limits.get(i).copied().flatten().map(|(a, b)| if j.kind.is_angle(i) { [a.to_degrees(), b.to_degrees()] } else { [a, b] })})
                })
                .collect();
            let world = |o: &JointOrigin| s.doc.origin_world(&st, o).ok().map(|m| json!({"origin": [m[3][0], m[3][1], m[3][2]], "z": [m[2][0], m[2][1], m[2][2]]}));
            json!({"id": j.id, "name": j.name, "type": j.kind, "a": j.a.occurrence, "b": j.b.occurrence, "values": named,
                "flip": j.flip, "offset": j.offset, "suppressed": j.suppressed, "frame_a": world(&j.a), "frame_b": world(&j.b)})
        })
        .collect();
    let links: Vec<Value> =
        s.doc.assembly.links.iter().map(|l| json!({"a": l.a, "ia": l.ia, "b": l.b, "ib": l.ib, "ratio": l.ratio, "offset": l.offset})).collect();
    let origins: Vec<Value> = s.doc.assembly.origins.iter().map(|o| json!({"name": o.name, "occurrence": o.origin.occurrence})).collect();
    let mut out = sol_json(&sol);
    out["joints"] = json!(js);
    out["links"] = json!(links);
    out["origins"] = json!(origins);
    Ok(out)
}

fn solve_cmd(s: &mut Session, _p: &Value) -> Result<Value> {
    let sol = resolve(s);
    Ok(sol_json(&sol))
}

fn interference(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "InterferenceCheckCommand";
    let want = string_list(p, "bodies");
    let st = s.world_state();
    let bodies: Vec<&solvecraft_doc::ModelBody> =
        st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).filter(|b| !b.body.is_mesh()).collect();
    if bodies.len() > 200 {
        return Err(bad(cmd, "too many bodies (200 at most)"));
    }
    let mut hits = Vec::new();
    for (i, a) in bodies.iter().enumerate() {
        for b in bodies.iter().skip(i + 1) {
            let (ba, bb) = (a.mesh().bounds(), b.mesh().bounds());
            let apart = ba.max.x < bb.min.x
                || bb.max.x < ba.min.x
                || ba.max.y < bb.min.y
                || bb.max.y < ba.min.y
                || ba.max.z < bb.min.z
                || bb.max.z < ba.min.z;
            if apart {
                continue;
            }
            let Ok(Some(o)) = solvecraft_kernel::boolean(&a.body, &b.body, solvecraft_kernel::BoolOp::Intersect) else { continue };
            let Ok(m) = solvecraft_kernel::measure(&o) else { continue };
            if m.volume > 1e-9 * a.body.size().max(1.0).powi(3) {
                hits.push(json!({"a": a.name, "b": b.name, "volume_mm3": m.volume, "area_mm2": m.area, "center": m.centroid, "bbox": {"min": m.bbox.min, "max": m.bbox.max}}));
            }
        }
    }
    Ok(json!({"interferences": hits, "count": hits.len(), "checked": bodies.len()}))
}

#[cfg(test)]
#[path = "joints_tests.rs"]
mod tests;
