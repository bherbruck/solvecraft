//! Surface commands: Patch, Stitch, Thicken and Trim (surface bodies and turning them into
//! solids).

use serde_json::Value;
use solvecraft_doc::{FaceAt, FeatureKind, PlaneRef, ProfileSel, expr::Kind};

use super::CommandSpec;
use super::features::{add_feature, check_expr, feature_sketch, operation, plane_param, profiles};
use crate::params::{bad, bool_, expr, str_, string_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("surface.patch", "Patch", patch)
        .at("SURFACE", "CREATE")
        .icon("patch")
        .params("sketch?: id|name and profiles? (planar patches of sketch regions) | edges: [[x,y,z]] (points on a closed loop of a body's edges) and body?; name?, body_name?"),
    CommandSpec::new("surface.stitch", "Stitch", stitch)
        .at("SURFACE", "MODIFY")
        .icon("stitch")
        .params("bodies: [names] (surfaces sewn into the first; a solid when they close); tolerance?: expr (default: tight)"),
    CommandSpec::new("surface.thicken", "Thicken", thicken)
        .at("SURFACE", "CREATE")
        .icon("thicken")
        .params("bodies: [surface names]; thickness: expr (negative: against the normal); symmetric?: bool; operation?: new|join|cut|intersect; targets?"),
    CommandSpec::new("surface.extend", "Extend", extend)
        .at("SURFACE", "MODIFY")
        .icon("extend")
        .params("body: planar surface name; edges: [[x,y,z]] (points on straight edges to push out); distance: expr"),
    CommandSpec::new("surface.trim", "Trim", trim)
        .at("SURFACE", "MODIFY")
        .icon("trim")
        .params("body: surface name; plane: XY|XZ|YZ|plane name|{origin, normal} or tool: {body, point} (a face, its surface extended); keep: [x,y,z] (a point on the side to keep)"),
];

fn patch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.patch";
    let edges: Vec<solvecraft_geom::Vec3> = p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if !edges.is_empty() {
        if edges.len() > 10_000 {
            return Err(bad(cmd, "too many edges"));
        }
        let body = str_(p, "body").map(str::to_string);
        return add_feature(s, p, FeatureKind::Patch { sketch: None, profiles: ProfileSel::All, body, edges });
    }
    let sketch = feature_sketch(s, p, cmd)?;
    let profiles = profiles(p, cmd)?;
    add_feature(s, p, FeatureKind::Patch { sketch: Some(sketch), profiles, body: None, edges: Vec::new() })
}

fn bodies_param(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    Ok(bodies)
}

fn stitch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.stitch";
    let bodies = bodies_param(s, p, cmd)?;
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    add_feature(s, p, FeatureKind::Stitch { bodies, tolerance })
}

fn thicken(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.thicken";
    let bodies = bodies_param(s, p, cmd)?;
    let thickness = expr(p, "thickness").ok_or_else(|| bad(cmd, "`thickness` is required"))?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let kind = FeatureKind::Thicken {
        bodies,
        thickness,
        symmetric: bool_(p, "symmetric").unwrap_or(false),
        operation: operation(p, cmd)?,
        targets: string_list(p, "targets"),
    };
    add_feature(s, p, kind)
}

fn extend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.extend";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let edges: Vec<solvecraft_geom::Vec3> = p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, "`edges` must list points on the edges to extend"));
    }
    let distance = expr(p, "distance").ok_or_else(|| bad(cmd, "`distance` is required"))?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    add_feature(s, p, FeatureKind::SurfaceExtend { body, edges, distance })
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.trim";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let keep = p.get("keep").and_then(vec3).ok_or_else(|| bad(cmd, "`keep` must be [x, y, z] on the side to keep"))?;
    let (plane, tool) = match p.get("tool").filter(|t| !t.is_null()) {
        Some(t) => {
            let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`tool.body` must name a body"))?.to_string();
            let point = t.get("point").and_then(vec3).ok_or_else(|| bad(cmd, "`tool.point` must be [x, y, z] on the tool face"))?;
            (PlaneRef::Origin { name: "XY".into() }, Some(FaceAt { body: tb, point }))
        }
        None => (plane_param(s, p.get("plane"), cmd)?, None),
    };
    add_feature(s, p, FeatureKind::SurfaceTrim { body, plane, tool, keep })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }
    fn volume(s: &Session, name: &str) -> f64 {
        let st = s.world_state();
        solvecraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume
    }

    /// A rectangle patched, trimmed by a plane and thickened; a box's faces stitched back.
    #[test]
    fn patch_trim_thicken_and_stitch() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        let st = s.world_state();
        assert!(st.body("Sheet").unwrap().body.is_surface());
        run(&mut s, "surface.trim", json!({"body": "Sheet", "plane": {"origin": [3, 0, 0], "normal": [1, 0, 0]}, "keep": [8, 5, 0]}));
        run(&mut s, "surface.extend", json!({"body": "Sheet", "edges": [[10, 5, 0]], "distance": 5}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": "2 mm", "body_name": "Slab"}));
        let v = volume(&s, "Slab");
        assert!((v - 240.0).abs() < 1e-9, "{v}");
        // Stitch the patch of a box's open top back on.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cup"}));
        run(&mut s, "surface.patch", json!({"edges": [[25, 0, 10], [30, 5, 10], [25, 10, 10], [20, 5, 10]], "body": "Cup", "body_name": "Lid"}));
        assert!(s.world_state().body("Lid").unwrap().body.is_surface());
        assert!(s.execute("surface.thicken", &json!({"bodies": ["Nope"], "thickness": 1})).is_err());
        assert!(s.execute("surface.trim", &json!({"body": "Sheet"})).is_err());
    }
}
