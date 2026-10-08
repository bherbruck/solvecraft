//! Rib, Web, Emboss, Replace Face, Align and Remove.

use serde_json::Value;
use solvecraft_doc::FeatureKind;
use solvecraft_doc::expr::Kind;
use solvecraft_geom::Vec3;

use super::CommandSpec;
use super::features::{add_feature, check_expr, feature_sketch, plane_param, profiles};
use crate::params::{bad, bool_, expr, req_expr, str_, string_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FusionRibCommand", "Rib", rib)
        .at("SOLID", "CREATE")
        .icon("rib")
        .params("sketch, curves: [open curve ids, in order]; thickness (across the sketch plane); depth? (default: to the next face); flip?: bool (fill to the other side)"),
    CommandSpec::new("FusionWebCommand", "Web", web)
        .at("SOLID", "CREATE")
        .icon("rib")
        .params("sketch, curves: [open curve ids] (one wall each); thickness; depth? (default: to the next face); flip?: bool"),
    CommandSpec::new("EmbossCmd", "Emboss", emboss)
        .at("SOLID", "CREATE")
        .icon("emboss")
        .params("sketch (on a planar face), profiles?; depth; mode?: emboss|deboss (default emboss); targets?"),
    CommandSpec::new("FusionReplaceFaceCommand", "Replace Face", replace_face)
        .at("SOLID", "MODIFY")
        .icon("offset_face")
        .params("faces: [[x,y,z] points on planar faces]; target: plane (XY|XZ|YZ|construction plane|{origin, normal}) | target_face: [x,y,z] on a parallel planar face; body?"),
    CommandSpec::new("AlignCmd", "Align", align)
        .at("SOLID", "MODIFY")
        .icon("move")
        .params("bodies: [names]; from: [x,y,z] | from_face: [x,y,z]; to: [x,y,z] | to_face: [x,y,z] (faces also turn to meet); flip?: bool"),
    CommandSpec::new("SoftDeleteCommand", "Remove", remove).at("SOLID", "MODIFY").icon("delete").params("bodies: [names] (removed from here on in the timeline)"),
];

fn curves_param(p: &Value, cmd: &str) -> Result<Vec<String>> {
    let c = string_list(p, "curves");
    if c.is_empty() || c.len() > 1000 {
        return Err(bad(cmd, "`curves` must list 1…1000 sketch curve ids"));
    }
    Ok(c)
}

fn rib_like(s: &mut Session, p: &Value, cmd: &str, web: bool) -> Result<Value> {
    let sketch = feature_sketch(s, p, cmd)?;
    let curves = curves_param(p, cmd)?;
    {
        let st = s.model.state();
        let ss = st.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
        if let Some(c) = curves.iter().find(|c| ss.sketch.curve_index(c).is_none()) {
            return Err(bad(cmd, format!("no curve `{c}` in the sketch")));
        }
    }
    let thickness = req_expr(cmd, p, "thickness")?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let depth = expr(p, "depth");
    if let Some(d) = &depth {
        check_expr(s, d, Kind::Length, cmd, "depth")?;
    }
    let flip = bool_(p, "flip").unwrap_or(false);
    add_feature(s, p, FeatureKind::Rib { sketch, curves, thickness, depth, flip, web })
}

fn rib(s: &mut Session, p: &Value) -> Result<Value> {
    rib_like(s, p, "FusionRibCommand", false)
}

fn web(s: &mut Session, p: &Value) -> Result<Value> {
    rib_like(s, p, "FusionWebCommand", true)
}

fn emboss(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "EmbossCmd";
    let sketch = feature_sketch(s, p, cmd)?;
    let profiles = profiles(p, cmd)?;
    let depth = req_expr(cmd, p, "depth")?;
    check_expr(s, &depth, Kind::Length, cmd, "depth")?;
    let deboss = match str_(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("emboss") => false,
        Some("deboss") => true,
        Some(m) => return Err(bad(cmd, format!("unknown mode `{m}` (emboss or deboss)"))),
    };
    if let Some(fp) = p.get("face").and_then(vec3)
        && super::face::face_normal(s, fp).is_none()
    {
        return Err(bad(cmd, "not supported yet: embossing onto a curved face"));
    }
    add_feature(s, p, FeatureKind::Emboss { sketch, profiles, depth, deboss, targets: string_list(p, "targets") })
}

fn face_points(p: &Value, k: &str, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{k}` must list points [x, y, z]")))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, format!("`{k}` must list 1…1000 points")));
    }
    a.iter().map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{k}` must contain [x, y, z] points")))).collect()
}

fn replace_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionReplaceFaceCommand";
    let faces = face_points(p, "faces", cmd)?;
    for f in &faces {
        if super::face::face_normal(s, *f).is_none() {
            return Err(bad(cmd, "not supported yet: replacing a face that is not planar"));
        }
    }
    let target = match p.get("target_face").and_then(vec3) {
        Some(tp) => {
            let n = super::face::face_normal(s, tp).ok_or_else(|| bad(cmd, "the target face must be planar"))?;
            let plane = solvecraft_geom::Plane::from_normal(tp, n).ok_or_else(|| bad(cmd, "the target face's plane"))?;
            solvecraft_doc::PlaneRef::Custom { plane }
        }
        None => plane_param(s, p.get("target"), cmd)?,
    };
    let body = str_(p, "body").map(str::to_string);
    add_feature(s, p, FeatureKind::ReplaceFace { faces, target, body })
}

fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "AlignCmd";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list the bodies to move"));
    }
    {
        let st = s.model.state();
        if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
            return Err(bad(cmd, format!("no body `{b}`")));
        }
    }
    let pick = |s: &Session, pk: &str, fk: &str| -> Result<(Vec3, Option<Vec3>)> {
        if let Some(fp) = p.get(fk).and_then(vec3) {
            let n = super::face::face_normal(s, fp).ok_or_else(|| bad(cmd, format!("`{fk}` must be on a planar face")))?;
            return Ok((fp, Some(n)));
        }
        p.get(pk).and_then(vec3).map(|v| (v, None)).ok_or_else(|| bad(cmd, format!("give `{pk}` or `{fk}` as [x, y, z]")))
    };
    let (from, from_normal) = pick(s, "from", "from_face")?;
    let (to, to_normal) = pick(s, "to", "to_face")?;
    let (from_normal, to_normal) = match (from_normal, to_normal) {
        (Some(a), Some(b)) => (Some(a), Some(b)),
        _ => (None, None),
    };
    let flip = bool_(p, "flip").unwrap_or(false);
    add_feature(s, p, FeatureKind::Align { bodies, from, to, from_normal, to_normal, flip })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SoftDeleteCommand";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    add_feature(s, p, FeatureKind::Remove { bodies })
}

#[cfg(test)]
#[path = "features_more_tests.rs"]
mod tests;
