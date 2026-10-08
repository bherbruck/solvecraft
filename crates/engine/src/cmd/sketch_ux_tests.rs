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

// ---------------------------------------------------------------------------------------------
// Construction toggle

fn profiles(s: &mut Session) -> i64 {
    run(s, "sketch.solve", json!({}))["profiles"].as_i64().unwrap_or(-1)
}

fn curve_ids(v: &Value) -> Vec<String> {
    v["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn construction_toggle_takes_shapes_out_of_profiles_and_back() {
    let mut s = new_sketch();
    let rect = curve_ids(&run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [20, 10]})));
    let slot = curve_ids(&run(&mut s, "ShapeSlotCenterToCenter", json!({"p0": [40, 5], "p1": [60, 5], "width": 6})));
    let poly = curve_ids(&run(&mut s, "ShapePolygonInscribed", json!({"center": [0, 40], "radius": 8, "sides": 6})));
    let circ = curve_ids(&run(&mut s, "CircleCenterRadius", json!({"center": [40, 40], "radius": 5})));
    let ell = curve_ids(&run(&mut s, "CircleElipse", json!({"center": [70, 40], "major": [80, 40], "minor_radius": 4})));
    assert_eq!(profiles(&mut s), 5);
    for (shape, left) in [(&rect, 4), (&slot, 3), (&poly, 2), (&circ, 1), (&ell, 0)] {
        let r = run(&mut s, "sketch.construction", json!({ "curves": shape }));
        assert_eq!(r["construction"], true);
        assert_eq!(profiles(&mut s), left, "{shape:?}");
    }
    // Back again in one go.
    let all: Vec<String> = [rect, slot, poly, circ, ell].concat();
    let r = run(&mut s, "sketch.construction", json!({ "curves": all }));
    assert_eq!(r["construction"], false);
    assert_eq!(profiles(&mut s), 5);
    // A mixed selection becomes construction as a whole.
    run(&mut s, "sketch.construction", json!({"curves": ["l1"]}));
    let r = run(&mut s, "sketch.construction", json!({"curves": ["l1", "l2", "l3", "l4"]}));
    assert_eq!(r["construction"], true);
    assert!(sketch(&s).curves.iter().filter(|c| ["l1", "l2", "l3", "l4"].contains(&c.id.as_str())).all(|c| c.construction));
    assert!(s.execute("sketch.construction", &json!({"curves": ["nope"]})).is_err());
}

#[test]
fn projected_geometry_can_become_construction() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 5}));
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ProjectNewCmd", json!({"refs": [{"edge": [10, 0, 5]}]}));
    let id = sketch(&s).curves.iter().find(|c| c.link.is_some()).map(|c| c.id.clone()).expect("projected");
    let r = run(&mut s, "sketch.construction", json!({ "curves": [id.clone()] }));
    assert_eq!(r["changed"], 1);
    let sk = sketch(&s);
    assert!(sk.curves.iter().any(|c| c.id == id && c.construction && c.link.is_some()));
}

// ---------------------------------------------------------------------------------------------
// Two-entity constraints: the first pick stays, the second moves

/// Positions of an entity's points (and its radius).
fn snap(s: &Session, id: &str) -> (Vec<solvecraft_geom::Vec2>, Option<f64>) {
    let sk = sketch(s);
    if let Some(i) = sk.curve_index(id) {
        let pts = sk.curves[i].kind.point_ids().into_iter().filter_map(|p| sk.point(p)).collect();
        return (pts, sk.radius(i));
    }
    (vec![pt(s, id)], None)
}

fn same(a: &(Vec<solvecraft_geom::Vec2>, Option<f64>), b: &(Vec<solvecraft_geom::Vec2>, Option<f64>)) -> bool {
    a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(p, q)| p.dist(*q) < 1e-9) && a.1.zip(b.1).is_none_or(|(x, y)| (x - y).abs() < 1e-9)
}

/// A free line l1 (5,3)–(45,3) and a circle c1 at (20,18) with radius 5.
fn line_and_circle() -> Session {
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[5, 3], [45, 3]]}));
    run(&mut s, "CircleCenterRadius", json!({"center": [20, 18], "radius": 5}));
    s
}

/// Apply `cmd` with `a` then `b`; the first must not move, the second must.
fn check_order(mut s: Session, cmd: &str, a: &str, b: &str) {
    let (a0, b0) = (snap(&s, a), snap(&s, b));
    run(&mut s, cmd, json!({"a": a, "b": b}));
    assert!(same(&snap(&s, a), &a0), "{cmd}: first `{a}` moved: {a0:?} -> {:?}", snap(&s, a));
    assert!(!same(&snap(&s, b), &b0), "{cmd}: second `{b}` did not move");
}

#[test]
fn tangent_moves_only_the_second_pick() {
    // Line then circle: only the circle moves (and keeps its size).
    let mut s = line_and_circle();
    let l0 = snap(&s, "l1");
    run(&mut s, "ConstraintTangent", json!({"a": "l1", "b": "c1"}));
    assert!(same(&snap(&s, "l1"), &l0), "line moved: {:?}", snap(&s, "l1"));
    let c = snap(&s, "c1");
    assert!((c.1.unwrap_or(0.0) - 5.0).abs() < 1e-9 && (c.0[0].y - 8.0).abs() < 1e-6, "{c:?}");
    // Circle then line: only the line moves.
    check_order(line_and_circle(), "ConstraintTangent", "c1", "l1");
}

#[test]
fn a_fixed_second_pick_makes_the_first_move() {
    let mut s = line_and_circle();
    run(&mut s, "ConstraintFix", json!({"entity": "l1"}));
    let l0 = snap(&s, "l1");
    let c0 = snap(&s, "c1");
    run(&mut s, "ConstraintTangent", json!({"a": "c1", "b": "l1"}));
    assert!(same(&snap(&s, "l1"), &l0));
    assert!(!same(&snap(&s, "c1"), &c0));
    // Fixed first: the second moves as usual.
    let mut s = line_and_circle();
    run(&mut s, "ConstraintFix", json!({"entity": "c1"}));
    let c0 = snap(&s, "c1");
    run(&mut s, "ConstraintTangent", json!({"a": "c1", "b": "l1"}));
    assert!(same(&snap(&s, "c1"), &c0));
}

#[test]
fn two_fully_constrained_entities_conflict_and_nothing_moves() {
    let mut s = line_and_circle();
    run(&mut s, "ConstraintFix", json!({"entities": ["l1.start", "l1.end", "c1.center"]}));
    run(&mut s, "SketchDimension", json!({"entities": ["c1"], "type": "radius", "value": 5}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["curves"].as_array().is_some_and(|c| c.iter().all(|c| c["fully_constrained"] == true)), "{si}");
    let before = sketch(&s);
    let e = s.execute("ConstraintTangent", &json!({"a": "l1", "b": "c1"}));
    assert!(e.is_err(), "{e:?}");
    assert_eq!(sketch(&s), before);
}

#[test]
fn line_pairs_keep_the_first_line() {
    let lines = || {
        let mut s = new_sketch();
        run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [30, 2]]}));
        run(&mut s, "DrawPolyline", json!({"points": [[5, 20], [25, 35]]}));
        s
    };
    let len = |s: &Session, l: &str| {
        let p = snap(s, l).0;
        p[0].dist(p[1])
    };
    for cmd in ["ConstraintParallel", "ConstraintPerpendicular", "ConstraintEqual", "ConstraintCollinear"] {
        check_order(lines(), cmd, "l1", "l2");
        check_order(lines(), cmd, "l2", "l1");
        // The second line turns and slides; only Equal changes its length.
        let mut s = lines();
        let l0 = len(&s, "l2");
        run(&mut s, cmd, json!({"a": "l1", "b": "l2"}));
        if cmd != "ConstraintEqual" {
            assert!((len(&s, "l2") - l0).abs() < 1e-6, "{cmd}: {l0} -> {}", len(&s, "l2"));
        } else {
            assert!((len(&s, "l2") - len(&s, "l1")).abs() < 1e-6);
        }
    }
}

#[test]
fn circle_pairs_keep_the_first_circle() {
    let circles = || {
        let mut s = new_sketch();
        run(&mut s, "CircleCenterRadius", json!({"center": [0, 0], "radius": 5}));
        run(&mut s, "CircleCenterRadius", json!({"center": [30, 10], "radius": 8}));
        s
    };
    for cmd in ["ConstraintConcentric", "ConstraintEqual", "ConstraintTangent"] {
        check_order(circles(), cmd, "c1", "c2");
        check_order(circles(), cmd, "c2", "c1");
    }
    // Equal resizes the second to the first.
    let mut s = circles();
    run(&mut s, "ConstraintEqual", json!({"a": "c2", "b": "c1"}));
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
}

#[test]
fn point_constraints_keep_the_first_pick() {
    let pts = || {
        let mut s = new_sketch();
        run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [30, 0]]}));
        run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [20, 25]]}));
        s
    };
    check_order(pts(), "ConstraintCoincident", "l2.start", "l1.end");
    check_order(pts(), "ConstraintCoincident", "l1.end", "l2.start");
    // Curve picked first, point second: the point goes onto the line.
    check_order(pts(), "ConstraintCoincident", "l1", "l2.start");
    // Midpoint: the point stays, the line moves.
    let mut s = pts();
    let p0 = snap(&s, "l2.end");
    run(&mut s, "ConstraintMidPoint", json!({"point": "l2.end", "line": "l1"}));
    assert!(same(&snap(&s, "l2.end"), &p0));
    // Symmetry: the first point and the line stay, the second mirrors.
    let mut s = pts();
    run(&mut s, "DrawPoint", json!({"point": [3, 7]}));
    run(&mut s, "DrawPoint", json!({"point": [4, -9]}));
    let (a0, l0) = (snap(&s, "p1"), snap(&s, "l1"));
    run(&mut s, "ConstraintSymmetry", json!({"a": "p1", "b": "p2", "line": "l1"}));
    assert!(same(&snap(&s, "p1"), &a0) && same(&snap(&s, "l1"), &l0));
    assert!(pt(&s, "p2").dist(solvecraft_geom::Vec2::new(3.0, -7.0)) < 1e-6, "{:?}", pt(&s, "p2"));
}

// ---------------------------------------------------------------------------------------------
// Sketch Palette options

#[test]
fn palette_options_are_kept_with_each_sketch() {
    let mut s = new_sketch();
    let o = run(&mut s, "sketch.options", json!({}));
    assert_eq!(o["options"]["show_dimensions"], true);
    assert_eq!(o["options"]["slice"], false);
    let o = run(&mut s, "sketch.options", json!({"show_dimensions": false, "snap": false, "slice": true, "sketch_3d": true}));
    assert_eq!(o["options"]["show_dimensions"], false);
    assert_eq!(o["options"]["snap"], false);
    assert_eq!(o["options"]["slice"], true);
    let v = sketch(&s).view;
    assert!(v.hide_dimensions && v.no_snap && v.slice && v.three_d && !v.hide_points);
    // Another sketch starts with the defaults; the first keeps its options after finishing.
    let first = s.active_sketch.expect("sketch");
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "SketchCreate", json!({"plane": "XZ"}));
    assert_eq!(sketch(&s).view, solvecraft_sketch::SketchView::default());
    let o = run(&mut s, "sketch.options", json!({"sketch": first}));
    assert_eq!(o["options"]["show_dimensions"], false);
    // Saved with the design.
    let text = serde_json::to_string(&s.doc.sketch(first).expect("first").clone()).expect("json");
    let back: Sketch = serde_json::from_str(&text).expect("parse");
    assert!(back.view.slice);
    assert!(s.execute("sketch.options", &json!({"bogus": true})).is_err());
    // Undo restores the option.
    run(&mut s, "sketch.options", json!({"show_points": false}));
    run(&mut s, "UndoCommand", json!({}));
    assert!(!sketch(&s).view.hide_points);
}

// ---------------------------------------------------------------------------------------------
// Dragging

fn drag(s: &mut Session, e: &str, from: [f64; 2], to: [f64; 2]) {
    run(s, "sketch.drag", json!({"entity": e, "from": from, "to": to}));
}

fn v(x: f64, y: f64) -> solvecraft_geom::Vec2 {
    solvecraft_geom::Vec2::new(x, y)
}

#[test]
fn dragging_a_line_slides_it_and_an_end_keeps_the_other_end() {
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [40, 20]]}));
    drag(&mut s, "l1", [25.0, 15.0], [30.0, 25.0]);
    assert!(pt(&s, "l1.start").dist(v(15.0, 20.0)) < 1e-9 && pt(&s, "l1.end").dist(v(45.0, 30.0)) < 1e-9);
    drag(&mut s, "l1.end", [45.0, 30.0], [50.0, 0.0]);
    assert!(pt(&s, "l1.end").dist(v(50.0, 0.0)) < 1e-9);
    assert!(pt(&s, "l1.start").dist(v(15.0, 20.0)) < 1e-9);
}

#[test]
fn dragging_a_rectangle_side_moves_that_side_only() {
    let mut s = new_sketch();
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [10, 10], "p1": [40, 30]}));
    // l2 is the right side (x = 40): dragged right, it stays vertical, the left side stays.
    let left = snap(&s, "l4");
    drag(&mut s, "l2", [40.0, 20.0], [50.0, 20.0]);
    assert!(same(&snap(&s, "l4"), &left), "left side moved");
    let (a, b) = (pt(&s, "l2.start"), pt(&s, "l2.end"));
    assert!((a.x - 50.0).abs() < 1e-6 && (b.x - 50.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn dragging_circles_and_arcs() {
    let mut s = new_sketch();
    run(&mut s, "CircleCenterRadius", json!({"center": [0, 0], "radius": 5}));
    // The edge changes the radius about a fixed centre; the centre moves the circle.
    drag(&mut s, "c1", [5.0, 0.0], [0.0, 8.0]);
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    assert!(pt(&s, "c1.center").dist(v(0.0, 0.0)) < 1e-9);
    drag(&mut s, "c1.center", [0.0, 0.0], [20.0, 5.0]);
    assert!(pt(&s, "c1.center").dist(v(20.0, 5.0)) < 1e-9);
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    run(&mut s, "ArcCenterTwoPoint", json!({"center": [50, 0], "start": [60, 0], "end": [50, 10]}));
    drag(&mut s, "a1", [57.07, 7.07], [50.0 + 200f64.sqrt(), 200f64.sqrt()]);
    assert!(pt(&s, "a1.center").dist(v(50.0, 0.0)) < 1e-9);
    assert!((snap(&s, "a1").1.unwrap_or(0.0) - 20.0).abs() < 1e-9, "{:?}", snap(&s, "a1"));
    assert!(pt(&s, "a1.start").dist(v(70.0, 0.0)) < 1e-9, "{:?}", pt(&s, "a1.start"));
}

#[test]
fn constrained_geometry_resists_drags() {
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [40, 10]]}));
    run(&mut s, "ConstraintFix", json!({"entities": ["l1.start", "l1.end"]}));
    let l0 = snap(&s, "l1");
    drag(&mut s, "l1", [25.0, 10.0], [25.0, 30.0]);
    assert!(same(&snap(&s, "l1"), &l0));
    // A horizontal line dragged at its end stays horizontal; its length dimension holds.
    let mut s = new_sketch();
    run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [40, 10]]}));
    run(&mut s, "ConstraintHorizontalVertical", json!({"line": "l1"}));
    run(&mut s, "SketchDimension", json!({"entities": ["l1"], "value": 30}));
    drag(&mut s, "l1.end", [40.0, 10.0], [45.0, 20.0]);
    let (a, b) = (pt(&s, "l1.start"), pt(&s, "l1.end"));
    assert!((a.y - b.y).abs() < 1e-6 && (a.dist(b) - 30.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn fully_constrained_feedback_is_per_entity() {
    let mut s = new_sketch();
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [10, 10], "p1": [50, 35]}));
    run(&mut s, "CircleCenterRadius", json!({"center": [80, 20], "radius": 8}));
    run(&mut s, "SketchDimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "SketchDimension", json!({"entities": ["l2"], "value": 25}));
    run(&mut s, "SketchDimension", json!({"entities": ["origin", "l1.start"], "type": "horizontal", "value": 10}));
    run(&mut s, "SketchDimension", json!({"entities": ["origin", "l1.start"], "type": "vertical", "value": 10}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 3);
    for c in si["curves"].as_array().expect("curves") {
        assert_eq!(c["fully_constrained"], c["type"] == "line", "{c}");
    }
    // Colours follow: fully constrained lines in the fixed colour, the free circle in blue.
    let sk = sketch(&s);
    let st = s.model.state();
    let ss = st.sketch(s.active_sketch.expect("sketch")).expect("solved");
    let lines = crate::view::sketch_lines(&sk, &ss.plane, true, &ss.report.curve_determined);
    assert!(lines.iter().filter(|l| l.1 == crate::view::colors::SKETCH_FIXED).count() >= 4);
    assert!(lines.iter().any(|l| l.1 == crate::view::colors::SKETCH));
}
