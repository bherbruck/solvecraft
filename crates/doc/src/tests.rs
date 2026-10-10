use solvecraft_geom::{Vec2, Vec3};
use solvecraft_kernel::measure;
use solvecraft_sketch::{ConstraintKind, Sketch};

use crate::*;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// 40 x 30 rectangle with its width and height driven by `width` / d2.
fn plate_doc() -> (Document, u64) {
    let mut doc = Document::new("plate");
    doc.set_param("width", "40 mm", Some("mm"), None).unwrap();
    let mut sk = Sketch::new();
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(38.0, 0.5), Vec2::new(39.0, 31.0), Vec2::new(0.5, 29.0)];
    let ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let l: Vec<usize> = (0..4).map(|i| sk.add_line_pts(ids[i], ids[(i + 1) % 4], None).unwrap()).collect();
    use ConstraintKind::*;
    sk.add_constraint(Coincident { p: ids[0], q: 0 }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
    let d1 = doc.new_model_param("width", "mm");
    sk.add_constraint(Length { l: l[0], value: 40.0 }, Some(d1)).unwrap();
    let d2 = doc.new_model_param("30 mm", "mm");
    sk.add_constraint(Length { l: l[1], value: 30.0 }, Some(d2)).unwrap();
    let s = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            sketch: s,
            profiles: ProfileSel::All,
            extent: Extent {
                distance: "20".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                through_all: false,
                taper: None,
                to: None,
                to_offset: None,
                from: None,
            },
            operation: Operation::NewBody,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    (doc, s)
}

#[test]
fn parametric_plate_reevaluates() {
    let (mut doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    assert_eq!(st.bodies.len(), 1);
    assert_eq!(st.bodies[0].name, "Body1");
    assert!(rel(measure(&st.bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    assert!(st.sketches[0].report.fully_constrained());

    // No change: nothing recomputed.
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 0);

    doc.set_param("width", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 2);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 30000.0) < 1e-9);

    // Changing only the extrude distance re-evaluates only the extrude.
    if let Some(FeatureKind::Extrude { extent, .. }) = doc.features.get_mut(1).map(|f| &mut f.kind) {
        extent.distance = "width / 5".into();
    }
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 1);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 50.0 * 30.0 * 10.0) < 1e-9);
}

/// Setting a parameter back takes the earlier results from the cache: nothing is recomputed, but
/// the evaluation reports the features it restored (their geometry did change back).
#[test]
fn set_back_reports_restored_features() {
    let (mut doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));
    doc.set_param("width", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));
    doc.set_param("width", "40 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 2));
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    // No change: nothing recomputed and nothing restored.
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 0));
}

#[test]
fn box_fillet_cut_timeline() {
    let (mut doc, _) = plate_doc();
    doc.add_feature(FeatureKind::Fillet { edges: vec![Vec3::new(0.0, 0.0, 10.0)], radius: "3".into(), body: None, style: Default::default() }, None)
        .unwrap();
    let mut sk = Sketch::new();
    sk.add_circle(Vec2::new(20.0, 15.0), 5.0, None, None).unwrap();
    let s2 = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            sketch: s2,
            profiles: ProfileSel::Points { points: vec![Vec2::new(20.0, 15.0)] },
            extent: Extent {
                distance: "20".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                through_all: false,
                taper: None,
                to: None,
                to_offset: None,
                from: None,
            },
            operation: Operation::Cut,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let st = m.state();
    let v = measure(&st.bodies[0].body).unwrap().volume;
    let expect = 24000.0 - (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 20.0 - std::f64::consts::PI * 25.0 * 20.0;
    assert!(rel(v, expect) < 1e-3, "{v} vs {expect}");
    // Roll back to before the fillet.
    doc.marker = Some(2);
    m.evaluate(&doc);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    assert!(m.results[3].skipped);
}

#[test]
fn errors_are_reported_not_fatal() {
    let (mut doc, s) = plate_doc();
    doc.add_feature(
        FeatureKind::Revolve {
            sketch: s,
            profiles: ProfileSel::All,
            axis: AxisRef::SketchAxis { axis: "y".into() },
            angle: "90 deg".into(),
            operation: Operation::NewBody,
            targets: vec![],
            angle2: None,
            symmetric: false,
            to: None,
        },
        None,
    )
    .unwrap();
    doc.add_feature(FeatureKind::Fillet { edges: vec![Vec3::new(500.0, 0.0, 0.0)], radius: "1".into(), body: None, style: Default::default() }, None)
        .unwrap();
    doc.add_feature(
        FeatureKind::Box { corner: Vec3::ZERO, length: "nope".into(), width: "1".into(), height: "1".into(), operation: Operation::NewBody },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results[2].error.is_none(), "{:?}", m.results[2]);
    assert!(m.results[3].error.is_some());
    assert!(m.results[4].error.as_deref().unwrap_or("").contains("nope"));
    assert_eq!(m.state().bodies.len(), 2);
    let quarter = measure(&m.state().bodies[1].body).unwrap().volume;
    // Revolving the 40x30 rectangle (x in [0,40]) 90° about sketch y: π/4 · 40² · 30.
    assert!(rel(quarter, std::f64::consts::FRAC_PI_4 * 1600.0 * 30.0) < 1e-3, "{quarter}");
}

#[test]
fn params_and_serialization() {
    let (mut doc, _) = plate_doc();
    assert!(doc.set_param("bad name", "1", None, None).is_err());
    assert!(doc.set_param("mm", "1", None, None).is_err());
    assert!(doc.set_param("loop", "loop + 1", None, None).is_err());
    assert!(doc.remove_param("width").is_err(), "used by d1");
    doc.set_param("spare", "3", None, None).unwrap();
    doc.remove_param("spare").unwrap();
    let json = doc.to_json();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, doc);
    assert!(Document::from_json("{").is_err());
    let gone = doc.delete_feature(doc.features[0].id).unwrap();
    assert_eq!(gone.len(), 2, "the extrude goes with its sketch");
    assert!(doc.param("d1").is_none(), "unused model parameters are pruned");
}

#[test]
fn primitives_and_combine() {
    let mut doc = Document::new("p");
    doc.add_feature(
        FeatureKind::Box { corner: Vec3::ZERO, length: "10".into(), width: "10".into(), height: "10".into(), operation: Operation::NewBody },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Cylinder {
            base: Vec3::new(5.0, 5.0, 0.0),
            axis: Vec3::Z,
            radius: "2".into(),
            height: "20".into(),
            operation: Operation::NewBody,
        },
        None,
    )
    .unwrap();
    doc.add_feature(FeatureKind::Combine { target: "Body1".into(), tools: vec!["Body2".into()], operation: Operation::Cut, keep_tools: false }, None)
        .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    assert_eq!(st.bodies.len(), 1);
    assert!(rel(measure(&st.bodies[0].body).unwrap().volume, 1000.0 - std::f64::consts::PI * 40.0) < 1e-3);
}

#[test]
fn hostile_expressions_never_panic() {
    // A deterministic stream of nasty inputs: random tokens, deep nesting, huge numbers.
    let toks = [
        "(",
        ")",
        "+",
        "-",
        "*",
        "/",
        "^",
        ",",
        "1e308",
        "0",
        "-0",
        "mm",
        "deg",
        "x",
        "sqrt",
        "pow",
        "min",
        "max",
        "°",
        "1.2.3",
        "e",
        "pi",
        "in",
        "nan",
        "9999999999999999999999",
    ];
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..5000 {
        let n = (next() % 24) as usize;
        let s: String = (0..n).map(|_| toks[(next() % toks.len() as u64) as usize]).collect::<Vec<_>>().join(" ");
        let _ = crate::expr::eval_with(&s, &|n| {
            if n == "x" { Ok(crate::expr::Value::length(2.0)) } else { Err(crate::DocError::Expr("unknown".into())) }
        });
    }
    let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
    assert!(crate::expr::eval_with(&deep, &|_| Err(crate::DocError::Expr("x".into()))).is_err());
    assert!(crate::expr::eval_with("pow(2 mm, 3)", &|_| Err(crate::DocError::Expr("x".into()))).is_ok());
    assert!(crate::expr::eval_with("pow(2 mm, 300)", &|_| Err(crate::DocError::Expr("x".into()))).is_err());
}

#[test]
fn parameter_cycles_are_found() {
    use crate::expr::Kind;
    let list = vec![
        ("a".to_string(), "b + 1".to_string(), Kind::Length),
        ("b".to_string(), "c * 2".to_string(), Kind::Length),
        ("c".to_string(), "a".to_string(), Kind::Length),
        ("d".to_string(), "5".to_string(), Kind::Length),
        ("e".to_string(), "d + e".to_string(), Kind::Length),
    ];
    let c = crate::cycles(&list);
    assert_eq!(c.into_iter().collect::<Vec<_>>(), vec!["a", "b", "c", "e"]);
    let defs: Vec<crate::expr::ParamDef> =
        list.iter().map(|(n, e, _)| crate::expr::ParamDef { name: n.clone(), expr: e.clone(), unit: "mm".into() }).collect();
    let (vals, errs) = crate::expr::eval_params(&defs, 1.0);
    assert!(errs["a"].contains("circular reference: a → b → c → a"), "{}", errs["a"]);
    assert!(vals.contains_key("d") && errs.contains_key("a") && errs.contains_key("e"));
}

/// Features that use a sketch's points or regions go with it (not only profile and path users).
#[test]
fn input_sketches_cover_points_and_patches() {
    let hole: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "hole", "position": [0, 0, 0], "direction": [0, 0, -1], "diameter": "3",
        "points": {"sketch": 7, "ids": ["p1"]},
    }))
    .unwrap();
    assert_eq!(hole.input_sketches(), vec![7]);
    let patch: FeatureKind = serde_json::from_value(serde_json::json!({"type": "patch", "sketch": 9})).unwrap();
    assert_eq!(patch.input_sketches(), vec![9]);
    let pipe: FeatureKind = serde_json::from_value(serde_json::json!({"type": "pipe", "path_sketch": 72, "path": ["l1"], "diameter": "4"})).unwrap();
    assert_eq!(pipe.input_sketches(), vec![72]);
}

/// Guard for `FeatureKind::input_sketches`: a variant with a field that refers to a sketch
/// (a field named `*sketch*` holding an id, sketch points or loft sections) must not sit in the
/// arm that returns no sketches. New variants are caught by the compiler (the match has no
/// wildcard); this catches a sketch field added to an existing variant.
#[test]
fn input_sketches_lists_every_variant_with_a_sketch_field() {
    let src = include_str!("document.rs");
    let body = src.split("pub enum FeatureKind {").nth(1).and_then(|s| s.split("\n}\n").next()).unwrap();
    let mut with_sketch = Vec::new();
    let mut variant = "";
    for line in body.lines() {
        let t = line.trim();
        if line.starts_with("    ") && !line.starts_with("     ") && t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            variant = t.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
            continue;
        }
        let Some((name, ty)) = t.strip_prefix("pub ").unwrap_or(t).split_once(':') else { continue };
        let refers = (name.contains("sketch") && (ty.contains("u64") || ty.contains("SketchPoints")))
            || ty.contains("SketchPoints")
            || ty.contains("LoftSection");
        if refers && !line.trim_start().starts_with("//") && !with_sketch.contains(&variant) {
            with_sketch.push(variant);
        }
    }
    assert!(with_sketch.len() >= 10, "the FeatureKind parse found too few sketch users: {with_sketch:?}");
    let f = src.split("pub fn input_sketches(&self)").nth(1).unwrap();
    let empty_arm = f.split("=> Vec::new()").next().and_then(|s| s.rsplit("=> ").next()).unwrap();
    for v in with_sketch {
        assert!(
            !empty_arm.contains(&format!("K::{v} {{")),
            "FeatureKind::{v} has a sketch field but input_sketches says it uses no sketch: deleting the sketch would leave it broken"
        );
    }
}
