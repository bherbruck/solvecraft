//! Random sequences of sketch commands: nothing panics, every sketch stays evaluable, and the
//! design survives a save/load round trip.

use serde_json::{Value, json};

use crate::{EngineError, Session};

/// Small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() % 1_000_000) as f64 / 1_000_000.0 * (hi - lo)
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> Option<&'a T> {
        if v.is_empty() { None } else { v.get((self.next() as usize) % v.len()) }
    }
    fn pt(&mut self) -> Value {
        json!([(self.f(-50.0, 50.0) * 10.0).round() / 10.0, (self.f(-50.0, 50.0) * 10.0).round() / 10.0])
    }
}

fn entities(s: &Session) -> (Vec<String>, Vec<String>, Vec<String>) {
    let Some(id) = s.active_sketch else { return Default::default() };
    let st = s.model.state();
    let Some(ss) = st.sketch(id) else { return Default::default() };
    let sk = &ss.sketch;
    let curves = sk.curves.iter().map(|c| c.id.clone()).collect();
    let points = sk.points.iter().map(|p| p.id.clone()).collect();
    let cons = sk.constraints.iter().map(|c| c.id.clone()).collect();
    (curves, points, cons)
}

#[test]
fn random_sketch_editing_never_breaks() {
    for seed in 1..=2u64 {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let mut s = Session::default();
        let _ = s.execute("PrimitiveBox", &json!({"length": 30, "width": 20, "height": 10}));
        let _ = s.execute("SketchCreate", &json!({"plane": {"face": [15, 10, 10]}}));
        for step in 0..120 {
            let (curves, points, cons) = entities(&s);
            let c = |r: &mut Rng| r.pick(&curves).cloned().unwrap_or_default();
            let p = |r: &mut Rng| r.pick(&points).cloned().unwrap_or_default();
            let (cmd, params) = match r.next() % 34 {
                0 => ("DrawPolyline", json!({"points": [r.pt(), r.pt(), r.pt()], "infer": true})),
                1 => ("CircleCenterRadius", json!({"center": r.pt(), "radius": r.f(0.5, 20.0)})),
                2 => ("ArcCenterTwoPoint", json!({"center": r.pt(), "start": r.pt(), "end": r.pt()})),
                3 => ("ShapeRectangleTwoPoint", json!({"p0": r.pt(), "p1": r.pt()})),
                4 => ("DrawSpline", json!({"points": [r.pt(), r.pt(), r.pt(), r.pt()]})),
                5 => ("CircleElipse", json!({"center": r.pt(), "major": r.pt(), "minor_radius": r.f(0.5, 10.0)})),
                6 => ("ConicCurveCmd", json!({"start": r.pt(), "end": r.pt(), "apex": r.pt(), "rho": r.f(0.1, 0.9)})),
                7 => ("TrimSketchCmd", json!({"curve": c(&mut r), "at": r.pt()})),
                8 => ("ExtendSketchCmd", json!({"curve": c(&mut r), "at": r.pt()})),
                9 => ("BreakSketchCmd", json!({"curve": c(&mut r), "at": r.pt()})),
                10 => ("FilletSketchCmd", json!({"point": p(&mut r), "radius": r.f(0.1, 5.0)})),
                11 => ("ChamferSketchEqualDistance", json!({"point": p(&mut r), "distance": r.f(0.1, 5.0)})),
                12 => ("Offset", json!({"curves": [c(&mut r)], "distance": r.f(-5.0, 5.0), "side": r.pt()})),
                13 => ("MirrorSketchCommand", json!({"entities": [c(&mut r)], "line": c(&mut r)})),
                14 => ("CircularSketchPatternCommand", json!({"entities": [c(&mut r)], "center": r.pt(), "count": 3})),
                15 => ("ConstraintCoincident", json!({"a": p(&mut r), "b": p(&mut r)})),
                16 => ("ConstraintTangent", json!({"a": c(&mut r), "b": c(&mut r)})),
                17 => ("ConstraintParallel", json!({"a": c(&mut r), "b": c(&mut r)})),
                18 => ("ConstraintSmooth", json!({"a": c(&mut r), "b": c(&mut r)})),
                19 => ("SketchDimension", json!({"entities": [c(&mut r)]})),
                20 => ("SketchDimension", json!({"entities": [p(&mut r), p(&mut r)], "driven": r.next().is_multiple_of(2)})),
                21 => ("sketch.delete", json!({"entities": [r.pick(&cons).cloned().unwrap_or_else(|| c(&mut r))]})),
                22 => ("sketch.move_point", json!({"point": p(&mut r), "to": r.pt()})),
                23 => ("ProjectNewCmd", json!({"refs": [{"edge": [r.f(0.0, 30.0), 0.0, 10.0]}]})),
                24 => ("sketch.break_link", json!({"entities": [c(&mut r)]})),
                25 => ("SketchConstrainer", json!({"entities": [c(&mut r), c(&mut r)]})),
                26 => ("SketchAutoConstraintAndDimCmd", json!({})),
                27 => ("MTextCmd", json!({"text": "Ab", "at": r.pt(), "height": r.f(1.0, 8.0)})),
                28 => ("sketch.toggle_driven", json!({"constraint": r.pick(&cons).cloned().unwrap_or_default()})),
                29 => ("ArcTangent", json!({"start": format!("{}.end", c(&mut r)), "end": r.pt()})),
                30 => ("SketchScaleCmd", json!({"entities": [c(&mut r)], "base": r.pt(), "factor": r.f(0.5, 2.0)})),
                31 => ("UndoCommand", json!({})),
                32 => ("ShapeArcSlotThreePoint", json!({"start": r.pt(), "through": r.pt(), "end": r.pt(), "width": r.f(0.5, 4.0)})),
                _ => ("RedoCommand", json!({})),
            };
            if let Err(EngineError::Internal(id, msg)) = s.execute(cmd, &params) {
                panic!("seed {seed} step {step}: {id} {params}: {msg}");
            }
            if s.active_sketch.is_none() {
                let _ = s.execute("SketchCreate", &json!({"plane": "XY"}));
            }
        }
        // The design round-trips.
        let back = solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(back, *s.doc, "seed {seed}");
    }
}
