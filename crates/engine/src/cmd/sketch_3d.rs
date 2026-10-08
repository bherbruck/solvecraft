//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, splines and
//! points may leave the sketch plane. They are kept as drawn 3D curves (wires with fit points):
//! drawn and snapped to, movable point by point, and usable as sweep and pipe paths.

use serde_json::{Value, json};
use solvecraft_geom::Vec3;

use super::sketch::edit;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, req_vec3, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.line3d", "3D Line", line3d)
        .enabled(in_sketch)
        .params("points: [[x,y,z]…] world points (2 or more): one 3D line per segment, joined end to end (needs 3D Sketch on)"),
    CommandSpec::new("sketch.spline3d", "3D Spline", spline3d)
        .enabled(in_sketch)
        .params("points: [[x,y,z]…] world fit points (2 or more): a smooth 3D curve through them (needs 3D Sketch on)"),
    CommandSpec::new("sketch.point3d", "3D Point", point3d).enabled(in_sketch).params("point: [x,y,z] world point (needs 3D Sketch on)"),
    CommandSpec::new("sketch.move3d", "Move 3D Point", move3d)
        .enabled(in_sketch)
        .params("wire: drawn 3D curve or point id, index?: fit point (default 0), to: [x,y,z] | by: [dx,dy,dz]; points joined to it move too"),
];

fn points(cmd: &str, p: &Value, min: usize) -> Result<Vec<Vec3>> {
    let a = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must list [x,y,z] points"))?;
    if a.len() < min || a.len() > solvecraft_sketch::MAX_FIT_POINTS {
        return Err(bad(cmd, format!("`points` needs {min} to {} points", solvecraft_sketch::MAX_FIT_POINTS)));
    }
    a.iter().map(|v| vec3(v).ok_or_else(|| bad(cmd, "a point must be [x, y, z]"))).collect()
}

fn needs_3d(sk: &solvecraft_sketch::Sketch, cmd: &str) -> Result<()> {
    if sk.view.three_d { Ok(()) } else { Err(bad(cmd, "turn on 3D Sketch (sketch.options sketch_3d: true) to draw off the sketch plane")) }
}

fn line3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.line3d";
    let pts = points(cmd, p, 2)?;
    let (ids, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        pts.windows(2).map(|w| sk.add_drawn_wire(w).map_err(Into::into)).collect::<Result<Vec<String>>>()
    })?;
    Ok(json!({"curves": ids, "sketch": info}))
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spline3d";
    let pts = points(cmd, p, 2)?;
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire(&pts)?)
    })?;
    Ok(json!({"curves": [id], "sketch": info}))
}

fn point3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point3d";
    let at = req_vec3(cmd, p, "point")?;
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire(&[at])?)
    })?;
    Ok(json!({"point": id, "sketch": info}))
}

fn move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move3d";
    let wire = p.get("wire").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`wire` must be a 3D curve or point id"))?.to_string();
    let index = p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    let to = p.get("to").and_then(vec3);
    let by = p.get("by").and_then(vec3);
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        let from = sk
            .wire_index(&wire)
            .and_then(|w| sk.wires.get(w))
            .filter(|w| !w.fit.is_empty())
            .and_then(|w| w.fit.get(index))
            .copied()
            .ok_or_else(|| bad(cmd, format!("`{wire}` has no drawn point {index}")))?;
        let to = match (to, by) {
            (Some(t), _) => t,
            (None, Some(d)) => from + d,
            _ => return Err(bad(cmd, "give `to` or `by`")),
        };
        Ok(sk.move_drawn_point(&wire, index, to)?)
    })?;
    Ok(json!({"moved": n, "sketch": info}))
}
