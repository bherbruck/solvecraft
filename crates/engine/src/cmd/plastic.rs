//! Plastic (enclosure) features: Boss, Lip / Groove and Snap Fit.

use serde_json::Value;
use solvecraft_doc::FeatureKind;
use solvecraft_doc::expr::Kind;
use solvecraft_geom::Vec3;

use super::CommandSpec;
use super::features::{add_feature, check_expr};
use crate::params::{bad, bool_, expr, req_expr, str_, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FusionBossCommand", "Boss", boss).at("PLASTIC", "CREATE").icon("boss").params(
        "position: [x,y,z] on a face; direction? (default: out of the face); diameter, height; hole_diameter?, hole_depth? (default: to the face); \
         draft? (angle), fillet? (root radius); ribs? (count), rib_thickness?, rib_length? (out from the wall), rib_offset? (below the top); body?",
    ),
    CommandSpec::new("FusionLipCommand", "Lip", lip).at("PLASTIC", "CREATE").icon("lip").params(
        "face: [x,y,z] on the rim face of a shelled body; width, height; type?: lip|groove; gap? (groove clearance); side?: inside|outside (which rim edge); body?",
    ),
    CommandSpec::new("FusionSnapFitCommand", "Snap Fit", snap_fit).at("PLASTIC", "CREATE").icon("snap").params(
        "position: [x,y,z] on a face; direction? (default: out of the face); hook: [x,y,z] (the catch's direction); length, thickness, width, catch_depth, catch_length; body?",
    ),
];

fn checked(s: &Session, p: &Value, k: &str, kind: Kind, cmd: &str) -> Result<Option<String>> {
    match expr(p, k) {
        Some(e) => {
            check_expr(s, &e, kind, cmd, k)?;
            Ok(Some(e))
        }
        None => Ok(None),
    }
}

fn required(s: &Session, p: &Value, k: &str, kind: Kind, cmd: &str) -> Result<String> {
    let e = req_expr(cmd, p, k)?;
    check_expr(s, &e, kind, cmd, k)?;
    Ok(e)
}

/// A point on a face and the direction out of it.
fn placed(s: &Session, p: &Value, cmd: &str) -> Result<(Vec3, Vec3)> {
    let at = p.get("position").and_then(vec3).ok_or_else(|| bad(cmd, "`position` must be a point [x, y, z] on a face"))?;
    let dir = match p.get("direction") {
        Some(v) => vec3(v).and_then(|d| d.normalized()).ok_or_else(|| bad(cmd, "`direction` must be a non-zero [x, y, z]"))?,
        None => super::face::face_normal(s, at).ok_or_else(|| bad(cmd, "no planar face there (give `direction`)"))?,
    };
    Ok((at, dir))
}

fn boss(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionBossCommand";
    let (position, direction) = placed(s, p, cmd)?;
    let kind = FeatureKind::Boss {
        position,
        direction,
        diameter: required(s, p, "diameter", Kind::Length, cmd)?,
        height: required(s, p, "height", Kind::Length, cmd)?,
        hole_diameter: checked(s, p, "hole_diameter", Kind::Length, cmd)?,
        hole_depth: checked(s, p, "hole_depth", Kind::Length, cmd)?,
        draft: checked(s, p, "draft", Kind::Angle, cmd)?,
        fillet: checked(s, p, "fillet", Kind::Length, cmd)?,
        ribs: checked(s, p, "ribs", Kind::Unitless, cmd)?,
        rib_thickness: checked(s, p, "rib_thickness", Kind::Length, cmd)?,
        rib_length: checked(s, p, "rib_length", Kind::Length, cmd)?,
        rib_offset: checked(s, p, "rib_offset", Kind::Length, cmd)?,
        body: str_(p, "body").map(str::to_string),
    };
    add_feature(s, p, kind)
}

fn lip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionLipCommand";
    let face = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on the rim face"))?;
    let groove = match str_(p, "type").map(str::to_ascii_lowercase).as_deref() {
        None | Some("lip") => false,
        Some("groove") => true,
        Some(o) => return Err(bad(cmd, format!("unknown type `{o}` (lip or groove)"))),
    };
    let outside = match str_(p, "side").map(str::to_ascii_lowercase).as_deref() {
        None | Some("inside") => false,
        Some("outside") => true,
        Some(o) => return Err(bad(cmd, format!("unknown side `{o}` (inside or outside)"))),
    };
    let kind = FeatureKind::Lip {
        face,
        width: required(s, p, "width", Kind::Length, cmd)?,
        height: required(s, p, "height", Kind::Length, cmd)?,
        groove,
        gap: checked(s, p, "gap", Kind::Length, cmd)?,
        outside: outside || bool_(p, "outside").unwrap_or(false),
        body: str_(p, "body").map(str::to_string),
    };
    add_feature(s, p, kind)
}

fn snap_fit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionSnapFitCommand";
    let (position, direction) = placed(s, p, cmd)?;
    let hook = p.get("hook").and_then(vec3).and_then(|h| h.normalized()).ok_or_else(|| bad(cmd, "`hook` must be the catch's direction [x, y, z]"))?;
    if hook.cross(direction).len() < 1e-6 {
        return Err(bad(cmd, "`hook` must point across the arm"));
    }
    let kind = FeatureKind::SnapFit {
        position,
        direction,
        hook,
        length: required(s, p, "length", Kind::Length, cmd)?,
        thickness: required(s, p, "thickness", Kind::Length, cmd)?,
        width: required(s, p, "width", Kind::Length, cmd)?,
        catch_depth: required(s, p, "catch_depth", Kind::Length, cmd)?,
        catch_length: required(s, p, "catch_length", Kind::Length, cmd)?,
        body: str_(p, "body").map(str::to_string),
    };
    add_feature(s, p, kind)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-12)
    }

    #[test]
    fn boss_with_hole_fillet_and_ribs() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 60, "width": 60, "height": 3, "body_name": "Plate"}));
        run(&mut s, "FusionBossCommand", json!({"position": [30, 30, 3], "diameter": 10, "height": 12, "hole_diameter": 4}));
        let want = 10800.0 + PI * 25.0 * 12.0 - PI * 4.0 * 12.0;
        assert!(rel(volume(&mut s), want) < 1e-3, "{} {want}", volume(&mut s));
        assert_eq!(run(&mut s, "MeasureCommand", json!({}))["body_count"], 1);
        // Ribs add t·l·h each.
        let mut t = Session::default();
        run(&mut t, "PrimitiveBox", json!({"length": 60, "width": 60, "height": 3}));
        run(
            &mut t,
            "FusionBossCommand",
            json!({"position": [30, 30, 3], "diameter": 10, "height": 12, "ribs": 4, "rib_thickness": 1.5, "rib_length": 6, "rib_offset": 2}),
        );
        let ribs = 4.0 * 1.5 * 6.0 * 10.0;
        // Each rib also fills a sliver between its flat inner end and the round wall.
        let v = volume(&mut t) - 10800.0 - PI * 25.0 * 12.0;
        assert!(v > ribs && v < ribs * 1.05, "{v} {ribs}");
        let mut u = Session::default();
        run(&mut u, "PrimitiveBox", json!({"length": 60, "width": 60, "height": 3}));
        run(&mut u, "FusionBossCommand", json!({"position": [30, 30, 3], "diameter": 10, "height": 12, "fillet": 2, "draft": 1}));
        assert!(volume(&mut u) > 10800.0 + PI * 25.0 * 10.0);
        assert!(u.execute("FusionBossCommand", &json!({"position": [30, 30, 3], "diameter": 10, "height": 12, "hole_diameter": 20})).is_err());
    }

    #[test]
    fn lip_and_groove_on_a_shelled_box() {
        let shelled = || {
            let mut s = Session::default();
            run(&mut s, "PrimitiveBox", json!({"length": 80, "width": 50, "height": 30}));
            run(&mut s, "FusionShellBodyCommand", json!({"faces": [[40, 25, 30]], "thickness": 2}));
            s
        };
        let mut s = shelled();
        let v0 = volume(&mut s);
        run(&mut s, "FusionLipCommand", json!({"face": [1, 25, 30], "width": 1, "height": 2}));
        // Inside band 1 wide along the inner edge (76 x 46): (76+2)(46+2) − 76·46 = 248, 2 high.
        assert!(rel(volume(&mut s) - v0, 248.0 * 2.0) < 1e-6, "{}", volume(&mut s) - v0);
        let mut g = shelled();
        run(&mut g, "FusionLipCommand", json!({"face": [1, 25, 30], "width": 1, "height": 2, "type": "groove", "side": "outside"}));
        // Outside band along the outer edge (80 x 50): 80·50 − 78·48 = 256, 2 deep.
        assert!(rel(v0 - volume(&mut g), 256.0 * 2.0) < 1e-6, "{}", v0 - volume(&mut g));
        assert!(g.execute("FusionLipCommand", &json!({"face": [40, 25, 0], "width": 1, "height": 2})).is_err(), "a face without an inner edge");
    }

    #[test]
    fn snap_fit_arm_and_catch() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 40, "height": 4}));
        let v0 = volume(&mut s);
        run(
            &mut s,
            "FusionSnapFitCommand",
            json!({"position": [20, 20, 4], "hook": [1, 0, 0], "length": 15, "thickness": 1.5, "width": 6, "catch_depth": 1, "catch_length": 3}),
        );
        let want = 1.5 * 15.0 * 6.0 + 0.5 * 1.0 * 3.0 * 6.0;
        assert!(rel(volume(&mut s) - v0, want) < 1e-6, "{}", volume(&mut s) - v0);
        let m = run(&mut s, "MeasureCommand", json!({}));
        assert!((m["bodies"][0]["bbox"]["max"][0].as_f64().unwrap_or(0.0) - 40.0).abs() < 1e-9);
        assert!(s.execute("FusionSnapFitCommand", &json!({"position": [5, 5, 4], "direction": [0, 0, 1], "hook": [0, 0, 1], "length": 15, "thickness": 1.5, "width": 6, "catch_depth": 1, "catch_length": 3})).is_err());
    }
}
