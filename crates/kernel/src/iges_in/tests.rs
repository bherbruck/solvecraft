use super::*;
use crate::{Body, BoolOp, ExportBody, StepHeader, boolean, box_solid, cylinder, fillet, iges_export_bodies, measure, sphere, torus};
use solvecraft_geom::Vec3;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

/// Export a body to IGES and read it back: same volume and area, its name and colour.
fn round_trip(b: &Body) -> crate::StepImport {
    let body = ExportBody { name: "Bracket left 1".into(), body: b, color: Some([1.0, 0.5, 0.0]) };
    let text = iges_export_bodies(&[body], &StepHeader { file_name: "t.igs".into(), ..Default::default() }).unwrap();
    for l in text.lines() {
        assert_eq!(l.chars().count(), 80, "{l}");
    }
    let imp = iges_import(&text).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1);
    let (m0, m1) = (measure(b).unwrap(), measure(&imp.bodies[0].body).unwrap());
    assert!(rel(m1.volume, m0.volume) < 1e-3, "volume {} vs {}", m1.volume, m0.volume);
    assert!(rel(m1.area, m0.area) < 1e-3, "area {} vs {}", m1.area, m0.area);
    assert!(imp.bodies[0].closed);
    assert_eq!(imp.bodies[0].name, "Bracket left 1");
    assert_eq!(imp.bodies[0].color, Some([1.0, 0.5, 0.0]));
    imp
}

#[test]
fn primitives_round_trip_through_iges() {
    round_trip(&box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap());
    round_trip(&cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 5.0, 12.0).unwrap());
    round_trip(&sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap());
    round_trip(&torus(Vec3::ZERO, 20.0, 4.0).unwrap());
}

#[test]
fn features_round_trip_through_iges() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
    round_trip(&holed);
    round_trip(&fillet(&plate, &[Vec3::new(20.0, 0.0, 10.0)], 3.0).unwrap());
}

/// Hostile files fail cleanly: garbage, truncation, missing and circular references, huge
/// counts.
#[test]
fn hostile_iges_never_panics() {
    let good = iges_export_bodies(
        &[ExportBody { name: "B".into(), body: &box_solid(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)).unwrap(), color: None }],
        &StepHeader::default(),
    )
    .unwrap();
    let mut cases: Vec<String> = vec![String::new(), "garbage".into(), "\u{0}\u{ff}".into(), good[..good.len() / 2].to_string()];
    // Every parameter line with its numbers bumped, scrambled or blown up.
    for (k, f) in [|s: &str| s.replace('1', "9"), |s: &str| s.replace(',', ";"), |s: &str| s.replace("0,", "999999999,")].iter().enumerate() {
        let t: String = good
            .lines()
            .map(|l| if l.as_bytes().get(72) == Some(&b'P') && l.len() >= 64 { format!("{}{}", f(&l[..64]), &l[64..]) } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = k;
        cases.push(t);
    }
    // A transformation and a composite curve that refer to themselves.
    cases.push(
        [
            format!("{:<72}S{:>7}", "", 1),
            format!("{:<72}G{:>7}", "1H,,1H;;", 1),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 124, 1, 0, 0, 0, 0, 1, 0, "00000000", 1),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 124, 0, 0, 1, 0, 0, 0, "", 0, 2),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 102, 2, 0, 0, 0, 0, 1, 0, "00000000", 3),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 102, 0, 0, 1, 0, 0, 0, "", 0, 4),
            format!("{:<64}{:>8}P{:>7}", "124,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.;", 1, 1),
            format!("{:<64}{:>8}P{:>7}", "102,1,3;", 3, 2),
            format!("{:<72}T{:>7}", "S      1G      1D      4P      2", 1),
        ]
        .join("\n"),
    );
    for c in cases {
        let r = std::panic::catch_unwind(|| iges_import(&c).map(|i| i.bodies.len()));
        assert!(r.is_ok(), "panicked on a hostile file");
    }
}

/// An IGES file from (type, form, parameters) entities, in order (pointers are 2k + 1).
fn iges_file(ents: &[(i64, i64, String)]) -> String {
    let mut out = vec![format!("{:<72}S{:>7}", "test", 1), format!("{:<72}G{:>7}", "1H,,1H;,,,,,,,,,,,1.,2,2HMM,,,,1.E-6;", 1)];
    let mut p_lines: Vec<(usize, String)> = Vec::new();
    let mut starts = Vec::new();
    for (k, (t, _, p)) in ents.iter().enumerate() {
        starts.push(p_lines.len() + 1);
        let text = format!("{t},{p};");
        let chars: Vec<char> = text.chars().collect();
        for c in chars.chunks(64) {
            p_lines.push((2 * k + 1, c.iter().collect()));
        }
    }
    for (k, (t, form, _)) in ents.iter().enumerate() {
        let count = p_lines.iter().filter(|(d, _)| *d == 2 * k + 1).count();
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", t, starts[k], 0, 0, 0, 0, 0, 0, "00000000", 2 * k + 1));
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", t, 0, 0, count, form, 0, 0, "", 0, 2 * k + 2));
    }
    for (i, (d, l)) in p_lines.iter().enumerate() {
        out.push(format!("{l:<64}{d:>8}P{:>7}", i + 1));
    }
    out.push(format!("{:<72}T{:>7}", "", 1));
    out.join("\n")
}

/// A unit-cube-like box (10 × 20 × 30) made of six trimmed bilinear B-spline patches whose
/// boundaries are lines: sewn into a closed solid.
#[test]
fn trimmed_surfaces_sew_into_a_solid() {
    let c =
        |i: usize| -> [f64; 3] { [if i & 1 != 0 { 10.0 } else { 0.0 }, if i & 2 != 0 { 20.0 } else { 0.0 }, if i & 4 != 0 { 30.0 } else { 0.0 }] };
    // Faces as corner quads (counter-clockwise seen from outside).
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut ents: Vec<(i64, i64, String)> = Vec::new();
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    for q in quads {
        let (a, b, cc, d) = (c(q[0]), c(q[1]), c(q[2]), c(q[3]));
        // Patch: u from a to b, v from a to d (normal (b - a) × (d - a) outward).
        let surf = format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(a), fmt(b), fmt(d), fmt(cc));
        ents.push((128, 0, surf));
        let s_de = 2 * ents.len() - 1;
        let mut lines = Vec::new();
        for (p0, p1) in [(a, b), (b, cc), (cc, d), (d, a)] {
            ents.push((110, 0, format!("{},{}", fmt(p0), fmt(p1))));
            lines.push(2 * ents.len() - 1);
        }
        ents.push((102, 0, format!("4,{}", lines.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))));
        let comp = 2 * ents.len() - 1;
        ents.push((142, 0, format!("0,{s_de},0,{comp},2")));
        let cos = 2 * ents.len() - 1;
        ents.push((144, 0, format!("{s_de},1,0,{cos}")));
    }
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6, "{} {}", m.volume, m.area);
}
