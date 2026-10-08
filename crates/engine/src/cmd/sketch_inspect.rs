//! Sketch inspection: curvature combs on sketch curves and model edges.

use serde_json::{Value, json};
use solvecraft_geom::{Seg2, Vec2, Vec3};

use super::CommandSpec;
use crate::params::{bad, num, string_list, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("FusionCurvatureCombAnalysisCommand", "Curvature Comb Analysis", curvature_comb)
    .at("SKETCH", "INSPECT")
    .icon("curvature_comb")
    .noundo()
    .params("curves?: [sketch curve ids] (sketch?: id|name, default active) | edges?: [[x,y,z] points on body edges]; density?: teeth per curve (default 40), scale?: comb length per unit curvature (default auto)")];

/// Curvature (signed, 1/mm) and unit normal of a segment at `t`.
fn seg_curvature(s: &Seg2, t: f64) -> (f64, Vec2) {
    let d1 = s.derivative(t);
    let h = 1e-5;
    let (ta, tb) = ((t - h).max(0.0), (t + h).min(1.0));
    let d2 = (s.derivative(tb) - s.derivative(ta)) / (tb - ta).max(1e-12);
    let l = d1.len().max(1e-300);
    let k = d1.cross(d2) / (l * l * l);
    (k, d1.perp() / l)
}

/// Teeth along a 2D curve: (foot, curvature, normal).
fn teeth_2d(segs: &[Seg2], density: usize) -> Vec<(Vec2, f64, Vec2)> {
    let per = density.div_ceil(segs.len().max(1)).max(2);
    let mut out = Vec::new();
    for s in segs {
        for i in 0..=per {
            let t = i as f64 / per as f64;
            let (k, n) = seg_curvature(s, t);
            out.push((s.point_at(t), k, n));
        }
    }
    out
}

/// Teeth along a 3D polyline (discrete curvature from three consecutive points).
fn teeth_3d(pts: &[Vec3]) -> Vec<(Vec3, f64, Vec3)> {
    let mut out = Vec::new();
    for w in pts.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let (ab, bc, ca) = (b - a, c - b, a - c);
        let area2 = ab.cross(bc).len();
        let den = ab.len() * bc.len() * ca.len();
        let k = if den > 1e-300 { 2.0 * area2 / den } else { 0.0 };
        // Towards the centre of curvature.
        let n = (a + c - b * 2.0).normalized().unwrap_or(Vec3::ZERO);
        out.push((b, k, n));
    }
    out
}

fn curvature_comb(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCurvatureCombAnalysisCommand";
    let density = num(p, "density").map(|d| d.clamp(2.0, 2000.0) as usize).unwrap_or(40);
    let st = s.model.state();
    let mut combs: Vec<(String, Vec<(Vec3, f64, Vec3)>)> = Vec::new();
    let curves = string_list(p, "curves");
    if !curves.is_empty() {
        let id = match p.get("sketch") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
            _ => s.active_sketch,
        }
        .ok_or_else(|| bad(cmd, "no such sketch (and none is active)"))?;
        let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
        for c in curves.iter().take(1000) {
            let ci = ss.sketch.curve_index(c).ok_or_else(|| bad(cmd, format!("unknown curve `{c}`")))?;
            let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
            let teeth = teeth_2d(&segs, density).into_iter().map(|(q, k, n)| (ss.plane.to_world(q), k, ss.plane.dir_to_world(n))).collect();
            combs.push((c.clone(), teeth));
        }
    }
    if let Some(edges) = p.get("edges").and_then(Value::as_array) {
        for e in edges.iter().take(1000) {
            let at = vec3(e).ok_or_else(|| bad(cmd, "`edges` must be points [x,y,z]"))?;
            let mut best: Option<(f64, Vec<Vec3>)> = None;
            for b in &st.bodies {
                let m = b.mesh();
                for (i, pts) in m.edges.iter().enumerate() {
                    if m.seams.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    let d = pts.windows(2).map(|w| at.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
                    if best.as_ref().is_none_or(|x| d < x.0) {
                        best = Some((d, pts.clone()));
                    }
                }
            }
            let (_, pts) = best.ok_or_else(|| bad(cmd, "no edge there"))?;
            combs.push((format!("edge@{},{},{}", at.x, at.y, at.z), teeth_3d(&pts)));
        }
    }
    if combs.is_empty() {
        return Err(bad(cmd, "give `curves` or `edges`"));
    }
    // One scale for all combs: the longest tooth is a fifth of the largest curve's size.
    let kmax = combs.iter().flat_map(|c| c.1.iter().map(|t| t.1.abs())).fold(0.0_f64, f64::max);
    let size = combs.iter().flat_map(|c| c.1.iter().map(|t| t.0)).fold(
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY)),
        |(lo, hi), q| (lo.min(q), hi.max(q)),
    );
    let diag = (size.1 - size.0).len();
    let scale = num(p, "scale").filter(|x| *x > 0.0).unwrap_or(if kmax > 1e-12 && diag.is_finite() { 0.2 * diag / kmax } else { 1.0 });
    let out: Vec<Value> = combs
        .iter()
        .map(|(id, teeth)| {
            let lines: Vec<[[f64; 3]; 2]> = teeth.iter().map(|(q, k, n)| [(*q).into(), (*q - *n * (k * scale)).into()]).collect();
            let kmax_c = teeth.iter().map(|t| t.1.abs()).fold(0.0_f64, f64::max);
            json!({
                "curve": id,
                "teeth": lines,
                "envelope": lines.iter().map(|l| l[1]).collect::<Vec<_>>(),
                "max_curvature": kmax_c,
                "min_radius": if kmax_c > 1e-12 { Value::from(1.0 / kmax_c) } else { Value::Null },
            })
        })
        .collect();
    Ok(json!({"combs": out, "scale": scale}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn curvature_combs() {
        let mut s = Session::default();
        s.execute("SketchCreate", &json!({"plane": "XY"})).unwrap();
        let c = s.execute("CircleCenterRadius", &json!({"center": [0, 0], "radius": 5})).unwrap()["curves"][0].clone();
        let l = s.execute("DrawPolyline", &json!({"points": [[0, 20], [10, 20]]})).unwrap()["curves"][0].clone();
        let r = s.execute("FusionCurvatureCombAnalysisCommand", &json!({"curves": [c, l], "density": 20})).unwrap();
        let circle = &r["combs"][0];
        assert!((circle["min_radius"].as_f64().unwrap() - 5.0).abs() < 1e-3, "{circle}");
        assert!(r["combs"][1]["min_radius"].is_null(), "a line is straight");
        // Circle teeth stand out on the convex side, away from the centre.
        let t = &circle["teeth"][3];
        let (a, b) = ((t[0][0].as_f64().unwrap(), t[0][1].as_f64().unwrap()), (t[1][0].as_f64().unwrap(), t[1][1].as_f64().unwrap()));
        assert!(b.0 * b.0 + b.1 * b.1 > a.0 * a.0 + a.1 * a.1);
        s.execute("SketchStop", &json!({})).unwrap();
        s.execute("PrimitiveCylinder", &json!({"base": [50, 0, 0], "radius": 8, "height": 10})).unwrap();
        let r = s.execute("FusionCurvatureCombAnalysisCommand", &json!({"edges": [[58, 0, 10]]})).unwrap();
        assert!((r["combs"][0]["min_radius"].as_f64().unwrap() - 8.0).abs() < 0.5, "{r}");
        assert!(s.execute("FusionCurvatureCombAnalysisCommand", &json!({})).is_err());
    }
}
