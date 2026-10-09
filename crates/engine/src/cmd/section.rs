//! Section Analysis: a view-only cut through the model by a plane (what is on the plane's
//! positive side is hidden). It changes no geometry and is not an undo step.

use serde_json::{Value, json};
use solvecraft_geom::{Plane, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, expr, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.section", "Section Analysis", section)
    .at("SOLID", "INSPECT")
    .icon("section")
    .noundo()
    .params("plane: XY|XZ|YZ | {origin, normal}; offset?: expr along the normal; flip?: bool; or clear: true")];

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section";
    if bool_(p, "clear").unwrap_or(false) {
        s.section = None;
        s.revision += 1;
        return Ok(json!({"section": null}));
    }
    let (origin, normal) = match p.get("plane") {
        Some(Value::String(n)) => {
            let pl = Plane::named(n).ok_or_else(|| bad(cmd, format!("unknown plane `{n}` (XY, XZ, YZ)")))?;
            (pl.origin, pl.normal())
        }
        Some(o @ Value::Object(_)) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let n = o.get("normal").and_then(vec3).and_then(|n| n.normalized()).ok_or_else(|| bad(cmd, "the plane needs a non-zero `normal`"))?;
            (origin, n)
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ or {origin, normal}")),
    };
    let offset = match expr(p, "offset") {
        Some(e) => s.doc.eval(&e, solvecraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("offset: {e}")))?,
        None => 0.0,
    };
    if !origin.is_finite() || !offset.is_finite() {
        return Err(bad(cmd, "the plane must be finite"));
    }
    let normal = if bool_(p, "flip").unwrap_or(false) { -normal } else { normal };
    let origin = origin + normal * offset * if bool_(p, "flip").unwrap_or(false) { -1.0 } else { 1.0 };
    s.section = Some((origin, normal));
    s.revision += 1;
    Ok(json!({"section": {"origin": origin, "normal": normal}}))
}
