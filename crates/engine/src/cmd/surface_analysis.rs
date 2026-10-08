//! Surface analyses: zebra stripes, draft angle and curvature map shading of the model's faces.
//! View state only: no geometry changes and no undo step.

use serde_json::{Value, json};
use solvecraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, num, vec3};
use crate::{Result, Session, SurfaceAnalysis};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FusionZebraAnalysisCommand", "Zebra Analysis", zebra)
        .at("SKETCH", "INSPECT")
        .icon("zebra")
        .noundo()
        .params("stripes?: number across the view (default 12); clear?: true turns the analysis off"),
    CommandSpec::new("FusionDraftAnalysisCommand", "Draft Analysis", draft)
        .at("SKETCH", "INSPECT")
        .icon("draft_analysis")
        .noundo()
        .params("pull?: [x,y,z] direction (default Z); angle?: degrees (default 1); clear?: true turns the analysis off"),
    CommandSpec::new("FusionEnvironmentMapAnalysisCommand", "Environment Map Analysis", |s, p| set(s, p, SurfaceAnalysis::Environment))
        .at("SKETCH", "INSPECT")
        .icon("environment_map")
        .noundo()
        .params("clear?: true turns the analysis off"),
    CommandSpec::new("FusionAccessibilityAnalysisCommand", "Accessibility Analysis", accessibility)
        .at("SKETCH", "INSPECT")
        .icon("accessibility")
        .noundo()
        .params("direction?: [x,y,z] the tool comes from (default Z, from above); clear?: true turns the analysis off"),
    CommandSpec::new("FusionCurvatureMapAnalysisCommand", "Curvature Map Analysis", curvature_map)
        .at("SKETCH", "INSPECT")
        .icon("curvature_map")
        .noundo()
        .params("radius?: mm shown red (default: a tenth of the model's size); clear?: true turns the analysis off"),
];

fn set(s: &mut Session, p: &Value, a: SurfaceAnalysis) -> Result<Value> {
    if bool_(p, "clear").unwrap_or(false) {
        s.analysis = None;
    } else {
        s.analysis = Some(a);
    }
    s.revision += 1;
    Ok(json!({ "analysis": s.analysis }))
}

fn zebra(s: &mut Session, p: &Value) -> Result<Value> {
    let stripes = num(p, "stripes").unwrap_or(12.0);
    if !(stripes.is_finite() && (1.0..=200.0).contains(&stripes)) {
        return Err(bad("FusionZebraAnalysisCommand", "`stripes` must be 1…200"));
    }
    set(s, p, SurfaceAnalysis::Zebra { stripes })
}

fn draft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionDraftAnalysisCommand";
    let pull = match p.get("pull") {
        Some(v) => vec3(v).and_then(Vec3::normalized).ok_or_else(|| bad(cmd, "`pull` must be a non-zero [x,y,z]"))?,
        None => Vec3::Z,
    };
    let angle = num(p, "angle").unwrap_or(1.0);
    if !(angle.is_finite() && (0.0..90.0).contains(&angle)) {
        return Err(bad(cmd, "`angle` must be 0…90 degrees"));
    }
    set(s, p, SurfaceAnalysis::Draft { pull, angle })
}

fn accessibility(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = match p.get("direction") {
        Some(v) => {
            vec3(v).and_then(Vec3::normalized).ok_or_else(|| bad("FusionAccessibilityAnalysisCommand", "`direction` must be a non-zero [x,y,z]"))?
        }
        None => Vec3::Z,
    };
    set(s, p, SurfaceAnalysis::Access { dir })
}

fn curvature_map(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCurvatureMapAnalysisCommand";
    let radius = match num(p, "radius") {
        Some(r) if r.is_finite() && r > 1e-6 && r < 1e9 => r,
        Some(_) => return Err(bad(cmd, "`radius` must be positive")),
        None => {
            let st = s.model.state();
            let size = st.bodies.iter().map(|b| b.mesh().bounds().diagonal()).fold(0.0_f64, f64::max);
            if size > 0.0 { size / 10.0 } else { 10.0 }
        }
    };
    set(s, p, SurfaceAnalysis::Curvature { radius })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{Session, SurfaceAnalysis};

    #[test]
    fn surface_analyses_are_view_state() {
        let mut s = Session::default();
        s.execute("PrimitiveBox", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let undo = s.undo.len();
        s.execute("FusionZebraAnalysisCommand", &json!({"stripes": 20})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Zebra { stripes: 20.0 }));
        s.execute("FusionDraftAnalysisCommand", &json!({"pull": [0, 0, 2], "angle": 3})).unwrap();
        assert!(matches!(s.analysis, Some(SurfaceAnalysis::Draft { angle, .. }) if angle == 3.0));
        s.execute("FusionCurvatureMapAnalysisCommand", &json!({})).unwrap();
        assert!(matches!(s.analysis, Some(SurfaceAnalysis::Curvature { radius }) if (radius - 300f64.sqrt() / 10.0).abs() < 1e-6));
        s.execute("FusionEnvironmentMapAnalysisCommand", &json!({})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Environment));
        s.execute("FusionAccessibilityAnalysisCommand", &json!({"direction": [0, 0, -3]})).unwrap();
        assert_eq!(s.analysis, Some(SurfaceAnalysis::Access { dir: solvecraft_geom::Vec3::new(0.0, 0.0, -1.0) }));
        s.execute("FusionCurvatureMapAnalysisCommand", &json!({"clear": true})).unwrap();
        assert_eq!(s.analysis, None);
        assert_eq!(s.undo.len(), undo, "no undo steps");
        assert!(s.execute("FusionZebraAnalysisCommand", &json!({"stripes": 0})).is_err());
        assert!(s.execute("FusionDraftAnalysisCommand", &json!({"pull": [0, 0, 0]})).is_err());
    }
}
