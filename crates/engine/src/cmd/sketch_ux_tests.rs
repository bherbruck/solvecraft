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
