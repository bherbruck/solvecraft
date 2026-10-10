//! Rib, Web, Emboss, Replace Face, Align and Remove.

use serde_json::Value;
use solvecraft_doc::FeatureKind;
use solvecraft_doc::expr::Kind;
use solvecraft_geom::Vec3;

use super::CommandSpec;
use super::features::{add_feature, axis_dir_param, check_expr, feature_sketch, plane_param, point_param, profiles};
use crate::params::{bad, bool_, expr, req_expr, str_, string_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("SurfaceSculpt", "Boundary Fill", boundary_fill).at("SOLID", "CREATE").icon("boundary_fill").params(
        "tools: [bodies]; cells: [[x,y,z] a point inside each cell to fill (a cell: inside some tools, outside the rest)]; operation?: new|join|cut|intersect; remove_tools?: bool",
    ),
    CommandSpec::new("solid.coil", "Coil", coil).at("SOLID", "CREATE").icon("coil").params(
        "diameter; two of revolutions (or turns), height, pitch; section_size; section?: circular|square; section_position?: inside|center|outside; \
         base?: [x,y,z] (numbers in mm or length expressions; default: the origin); axis?: X|Y|Z|[x,y,z] (default Z); start_angle?; clockwise?: bool; operation?, targets?, name?, body_name?",
    ),
    CommandSpec::new("solid.rib", "Rib", rib)
        .at("SOLID", "CREATE")
        .icon("rib")
        .params("sketch, curves: [open curve ids, in order]; thickness (across the sketch plane); depth? (default: to the next face); flip?: bool (fill to the other side)"),
    CommandSpec::new("solid.web", "Web", web)
        .at("SOLID", "CREATE")
        .icon("rib")
        .params("sketch, curves: [open curve ids] (one wall each); thickness; depth? (default: to the next face); flip?: bool"),
    CommandSpec::new("solid.emboss", "Emboss", emboss)
        .at("SOLID", "CREATE")
        .icon("emboss")
        .params("sketch (on a planar face), profiles?; depth; mode?: emboss|deboss (default emboss); targets?"),
    CommandSpec::new("solid.replace_face", "Replace Face", replace_face)
        .at("SOLID", "MODIFY")
        .icon("offset_face")
        .params("faces: [[x,y,z] points on planar faces]; target: plane (XY|XZ|YZ|construction plane|{origin, normal}) | target_face: [x,y,z] on a parallel planar face; body?"),
    CommandSpec::new("solid.align", "Align", align)
        .at("SOLID", "MODIFY")
        .icon("move")
        .params("bodies: [names]; from: [x,y,z] | {snap: vertex|edge_mid|face_center|circle_center, at: [x,y,z]} | from_face: [x,y,z]; to: (the same) | to_face: [x,y,z] (faces also turn to meet); flip?: bool"),
    CommandSpec::new("solid.remove", "Remove", remove).at("SOLID", "MODIFY").icon("delete").params("bodies: [names] (removed from here on in the timeline)"),
];

fn coil(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.coil";
    let base = point_param(s, p, "base", cmd)?.unwrap_or_else(|| solvecraft_doc::point_expr(Vec3::ZERO));
    let axis = axis_dir_param(p, cmd)?;
    let diameter = req_expr(cmd, p, "diameter")?;
    check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    let turns = expr(p, "revolutions").or_else(|| expr(p, "turns"));
    let (height, pitch) = (expr(p, "height"), expr(p, "pitch"));
    let (turns, pitch) = match (turns, height, pitch) {
        (Some(n), _, Some(pt)) => (n, pt),
        (Some(n), Some(h), None) => (n.clone(), format!("({h}) / ({n})")),
        (None, Some(h), Some(pt)) => (format!("({h}) / ({pt})"), pt),
        _ => return Err(bad(cmd, "give two of `revolutions`, `height` and `pitch`")),
    };
    check_expr(s, &turns, Kind::Unitless, cmd, "revolutions")?;
    check_expr(s, &pitch, Kind::Length, cmd, "pitch")?;
    let section_size = req_expr(cmd, p, "section_size")?;
    check_expr(s, &section_size, Kind::Length, cmd, "section_size")?;
    let section = match str_(p, "section").map(str::to_ascii_lowercase).as_deref() {
        None | Some("circular" | "circle") => solvecraft_doc::CoilSection::Circular,
        Some("square") => solvecraft_doc::CoilSection::Square,
        Some(o) => return Err(bad(cmd, format!("unknown section `{o}` (circular or square)"))),
    };
    let position = match str_(p, "section_position").map(str::to_ascii_lowercase).as_deref() {
        None | Some("center" | "on" | "on_center") => 0,
        Some("inside") => -1,
        Some("outside") => 1,
        Some(o) => return Err(bad(cmd, format!("unknown section position `{o}` (inside, center or outside)"))),
    };
    let start_angle = expr(p, "start_angle");
    if let Some(a) = &start_angle {
        check_expr(s, a, Kind::Angle, cmd, "start_angle")?;
    }
    let clockwise = bool_(p, "clockwise").unwrap_or(false);
    let operation = super::features::operation(p, cmd)?;
    add_feature(
        s,
        p,
        FeatureKind::Coil {
            base,
            axis,
            diameter,
            pitch,
            turns,
            section_size,
            section,
            position,
            start_angle,
            clockwise,
            operation,
            targets: string_list(p, "targets"),
        },
    )
}

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
    rib_like(s, p, "solid.rib", false)
}

fn web(s: &mut Session, p: &Value) -> Result<Value> {
    rib_like(s, p, "solid.web", true)
}

fn emboss(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.emboss";
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
    let cmd = "solid.replace_face";
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
    let cmd = "solid.align";
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
        // A snap point: {snap: vertex|edge_mid|face_center|circle_center, at: [x,y,z]}.
        if let Some(o) = p.get(pk).filter(|v| v.is_object()) {
            let at = o.get("at").and_then(vec3).ok_or_else(|| bad(cmd, format!("`{pk}.at` must be [x, y, z]")))?;
            let kind = o.get("snap").and_then(Value::as_str).unwrap_or("vertex");
            return snap_point(s, kind, at).map(|q| (q, None)).ok_or_else(|| bad(cmd, format!("no {kind} to snap to near `{pk}`")));
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
    let cmd = "solid.remove";
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

/// A snap point near `at` on the model: the nearest vertex, edge midpoint, planar face's centre
/// or circular edge's centre (the end of a cylindrical face).
pub(super) fn snap_point(s: &Session, kind: &str, at: Vec3) -> Option<Vec3> {
    let st = s.world_state();
    let near = |pts: Vec<Vec3>| pts.into_iter().min_by(|a, b| a.dist(at).total_cmp(&b.dist(at)));
    match kind {
        "vertex" => near(
            st.bodies
                .iter()
                .flat_map(|b| b.mesh().edges.iter().flat_map(|e| [e.first().copied(), e.last().copied()]).flatten().collect::<Vec<_>>())
                .collect(),
        ),
        "edge_mid" => {
            let d = |e: &solvecraft_doc::kernel::EdgeInfo| e.points.windows(2).map(|w| at.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min);
            st.bodies
                .iter()
                .filter_map(|b| b.body.edges((b.body.size() * 2e-3).max(1e-3)).ok())
                .flatten()
                .min_by(|a, b| d(a).total_cmp(&d(b)))
                .map(|e| e.mid)
        }
        "face_center" => st.bodies.iter().find_map(|b| {
            let i = solvecraft_doc::appearance::face_index_at(b, at)?;
            b.body.faces((b.body.size() * 2e-3).max(1e-3)).ok()?.into_iter().find(|f| f.index == i).map(|f| f.centroid)
        }),
        "circle_center" => st.bodies.iter().find_map(|b| {
            let c = solvecraft_doc::kernel::cylinder_face_at(&b.body, at)?;
            let along = (at - c.axis_point).dot(c.axis);
            let end = if (along - c.start).abs() <= (c.end - along).abs() { c.start } else { c.end };
            Some(c.axis_point + c.axis * end)
        }),
        _ => None,
    }
}

fn boundary_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SurfaceSculpt";
    let tools = string_list(p, "tools");
    let st = s.model.state();
    if tools.is_empty() || tools.len() > 64 || tools.iter().any(|t| st.body(t).is_none()) {
        return Err(bad(cmd, "`tools` must list 1…64 bodies"));
    }
    let cells: Vec<Vec3> = p.get("cells").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if cells.is_empty() || cells.len() > 1000 {
        return Err(bad(cmd, "`cells` must list 1…1000 points [x, y, z], one inside each cell"));
    }
    let operation = super::features::operation(p, cmd)?;
    add_feature(s, p, FeatureKind::BoundaryFill { tools, cells, operation, remove_tools: bool_(p, "remove_tools").unwrap_or(false) })
}
