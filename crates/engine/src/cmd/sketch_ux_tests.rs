//! Sketch interaction tests: dimension placement, Fix/Unfix, the construction toggle, and
//! which entity moves when a two-entity constraint is applied.

use serde_json::{Value, json};
use solvecraft_sketch::{DimFrame, Sketch, dim_frame, dim_layout};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> Sketch {
    let id = s.active_sketch.expect("a sketch is being edited");
    s.doc.sketch(id).expect("the sketch").clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    s
}

// ---------------------------------------------------------------------------------------------
// Dimensions

#[test]
fn every_dimension_kind_has_a_drawable_frame() {
    let mut s = new_sketch();
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [60, 40]}));
    run(&mut s, "CircleCenterRadius", json!({"center": [30, 20], "radius": 8}));
    run(&mut s, "ArcCenterTwoPoint", json!({"center": [100, 0], "start": [110, 0], "end": [100, 10]}));
    run(&mut s, "DrawPolyline", json!({"points": [[-40, 0], [-10, 0]]}));
    run(&mut s, "DrawPolyline", json!({"points": [[-40, 0], [-20, 25]]}));
    for (ents, ty) in [
        (json!(["l1"]), "auto"),
        (json!(["l2"]), "vertical"),
        (json!(["l1.start", "c1.center"]), "horizontal"),
        (json!(["c1"]), "auto"),
        (json!(["a1"]), "radius"),
        (json!(["a1"]), "arc_length"),
        (json!(["l5", "l6"]), "angle"),
    ] {
        run(&mut s, "SketchDimension", json!({"entities": ents, "type": ty}));
    }
    run(&mut s, "SketchDimension", json!({"entities": ["c1.center", "l2"], "driven": true}));
    let sk = sketch(&s);
    let dims: Vec<_> = sk.constraints.iter().filter(|c| c.kind.is_dimension()).collect();
    assert_eq!(dims.len(), 8);
    let mut kinds = Vec::new();
    for c in dims {
        let f = dim_frame(&sk, &c.kind).unwrap_or_else(|| panic!("no frame for {c:?}"));
        let l = dim_layout(&f, None, 0.1, solvecraft_geom::Vec2::new(1.0, 0.5));
        assert!(!l.lines.is_empty() && !l.arrows.is_empty(), "{c:?}");
        assert!(l.text.is_finite());
        kinds.push(std::mem::discriminant(&f));
    }
    for f in [
        DimFrame::Linear { p0: Default::default(), p1: Default::default(), dir: Default::default() },
        DimFrame::Radial { center: Default::default(), radius: 1.0, diameter: true },
        DimFrame::Angular { vertex: Default::default(), d0: Default::default(), sweep: 1.0 },
        DimFrame::ArcLength { center: Default::default(), radius: 1.0, start: 0.0, sweep: 1.0 },
    ] {
        assert!(kinds.contains(&std::mem::discriminant(&f)));
    }
}

#[test]
fn dimension_text_is_stored_relative_to_the_dimension_and_follows_it() {
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [40, 0]]}));
    let d = run(&mut s, "SketchDimension", json!({"entities": ["l1"], "text_at": [20, -8]}));
    let param = d["param"].as_str().unwrap_or_default().to_string();
    let text = |s: &Session| {
        let sk = sketch(s);
        let c = sk.constraints.iter().find(|c| c.kind.is_dimension()).cloned().expect("dimension");
        let f = dim_frame(&sk, &c.kind).expect("frame");
        dim_layout(&f, c.text, 0.1, Default::default()).text
    };
    assert!(text(&s).dist(solvecraft_geom::Vec2::new(20.0, -8.0)) < 1e-9);
    // Moved by the parameter name; then the line moves and the text goes with it.
    run(&mut s, "sketch.dimension_text", json!({"dimension": param, "at": [30, 12]}));
    assert!(text(&s).dist(solvecraft_geom::Vec2::new(30.0, 12.0)) < 1e-9);
    run(&mut s, "sketch.move_point", json!({"point": "l1.start", "to": [0, 10]}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [40, 10]}));
    // Still 10 along and 12 across from the middle of the (moved) line.
    let sk = sketch(&s);
    let Some(DimFrame::Linear { p0, p1, dir }) = sk.constraints.iter().find_map(|c| dim_frame(&sk, &c.kind)) else { panic!("linear frame") };
    assert!(p0.y > 5.0, "the line moved: {p0:?}");
    let off = text(&s) - (p0 + p1) * 0.5;
    assert!((off.dot(dir) - 10.0).abs() < 1e-9 && (off.dot(dir.perp()) - 12.0).abs() < 1e-9, "{off:?}");
    // Reset puts it back to the default place; undo restores the drag.
    run(&mut s, "sketch.dimension_text", json!({"dimension": param, "reset": true}));
    assert!(sketch(&s).constraints.iter().all(|c| c.text.is_none()));
    assert!(s.execute("sketch.dimension_text", &json!({"dimension": "nope", "at": [0, 0]})).is_err());
    assert!(s.execute("sketch.dimension_text", &json!({"dimension": param, "at": [f64::NAN, 0]})).is_err());
}

#[test]
fn deleting_a_dimension_frees_its_parameter() {
    let mut s = new_sketch();
    run(&mut s, "CircleCenterRadius", json!({"center": [0, 0], "radius": 5}));
    let d = run(&mut s, "SketchDimension", json!({"entities": ["c1"]}));
    let param = d["param"].as_str().unwrap_or_default().to_string();
    let id = sketch(&s).constraints.iter().find(|c| c.param.as_deref() == Some(param.as_str())).map(|c| c.id.clone()).expect("dim");
    run(&mut s, "sketch.delete", json!({"entities": [id]}));
    assert!(sketch(&s).constraints.iter().all(|c| !c.kind.is_dimension()));
    assert!(s.doc.param(&param).is_none());
}

// ---------------------------------------------------------------------------------------------
// Fix / Unfix

fn pt(s: &Session, id: &str) -> solvecraft_geom::Vec2 {
    let sk = sketch(s);
    sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap_or_else(|| panic!("no point {id}"))
}

fn dof(s: &mut Session) -> i64 {
    run(s, "sketch.inspect", json!({}))["dof"].as_i64().unwrap_or(-1)
}

#[test]
fn fixed_points_survive_drags_and_solves_and_unfix_frees_them() {
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [30, 0], [30, 20]]}));
    let free = dof(&mut s);
    let r = run(&mut s, "ConstraintFix", json!({"entity": "l1.end"}));
    assert_eq!(r["result"]["fixed"], true);
    assert_eq!(dof(&mut s), free - 2);
    let corner = pt(&s, "l1.end");
    // Dragging the fixed point does nothing; dragging its neighbours never moves it.
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [50, 50]}));
    assert_eq!(pt(&s, "l1.end"), corner);
    run(&mut s, "ConstraintHorizontalVertical", json!({"line": "l2", "mode": "vertical"}));
    run(&mut s, "sketch.move_point", json!({"point": "l2.end", "to": [40, 25]}));
    run(&mut s, "SketchDimension", json!({"entities": ["l1"], "value": 12}));
    assert!(pt(&s, "l1.end").dist(corner) < 1e-12, "{:?}", pt(&s, "l1.end"));
    assert!((pt(&s, "l1.start").dist(corner) - 12.0).abs() < 1e-6);
    // Unfix: the point moves again.
    let r = run(&mut s, "ConstraintFix", json!({"entity": "l1.end"}));
    assert_eq!(r["result"]["fixed"], false);
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [30, -5]}));
    assert!(pt(&s, "l1.end").dist(corner) > 1.0);
}

#[test]
fn fixing_curves_holds_their_points_and_radius() {
    let mut s = new_sketch();
    run(&mut s, "CircleCenterRadius", json!({"center": [10, 10], "radius": 5}));
    run(&mut s, "DrawPolyline", json!({"points": [[30, 0], [50, 0]]}));
    run(&mut s, "ArcCenterTwoPoint", json!({"center": [0, 40], "start": [10, 40], "end": [0, 50]}));
    run(&mut s, "CircleElipse", json!({"center": [60, 40], "major": [70, 40], "minor_radius": 4}));
    let free = dof(&mut s);
    // Multi-select fixes them all (circle 3, line 4, arc 5, ellipse 5 degrees of freedom).
    let r = run(&mut s, "ConstraintFix", json!({"entities": ["c1", "l1", "a1", "e1"]}));
    assert_eq!(r["result"]["changed"], 4);
    assert_eq!(dof(&mut s), free - 17);
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["curves"].as_array().is_some_and(|c| c.iter().all(|c| c["fixed"] == true)), "{si}");
    // A fixed circle keeps centre and radius; a dimension on it conflicts.
    run(&mut s, "sketch.move_point", json!({"point": "c1.center", "to": [0, 0]}));
    assert_eq!(pt(&s, "c1.center"), solvecraft_geom::Vec2::new(10.0, 10.0));
    assert!(s.execute("SketchDimension", &json!({"entities": ["c1"], "value": 20})).is_err());
    let r0 = sketch(&s).radius(0);
    assert_eq!(r0, Some(5.0));
    // Toggle without `fixed`: all fixed, so all are unfixed.
    let r = run(&mut s, "ConstraintFix", json!({"entities": ["c1", "l1", "a1", "e1"]}));
    assert_eq!(r["result"]["fixed"], false);
    assert_eq!(dof(&mut s), free);
    run(&mut s, "SketchDimension", json!({"entities": ["c1"], "type": "radius", "value": 7}));
    assert!((sketch(&s).radius(0).unwrap_or(0.0) - 7.0).abs() < 1e-9);
    // Mixed selection: one fixed, one free → both end up fixed.
    run(&mut s, "ConstraintFix", json!({"entity": "l1"}));
    let r = run(&mut s, "ConstraintFix", json!({"entities": ["l1", "l1.start", "a1"]}));
    assert_eq!(r["result"]["fixed"], true);
    assert!(sketch(&s).curves.iter().filter(|c| c.id == "l1" || c.id == "a1").all(|c| c.fixed));
}

#[test]
fn projected_geometry_counts_as_fixed() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 5}));
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    let r = run(&mut s, "ProjectNewCmd", json!({"refs": [{"edge": [10, 0, 5]}]}));
    let sk = sketch(&s);
    let ci = sk.curves.iter().position(|c| c.link.is_some()).unwrap_or_else(|| panic!("projected: {r}"));
    let id = sk.curves[ci].id.clone();
    assert!(sk.curve_locked(ci));
    let r = run(&mut s, "ConstraintFix", json!({"entity": id}));
    // Already fixed: the toggle unfixes, which projected geometry ignores.
    assert_eq!(r["result"]["changed"], 0);
    assert!(sketch(&s).curve_locked(ci));
}
