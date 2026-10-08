use std::f64::consts::PI;

use solvecraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn rect(w: f64, h: f64) -> Region2 {
    Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)]), holes: vec![] }
}

#[test]
fn box_extrude_measures() {
    let b = extrude(&Plane::XY, &[rect(40.0, 30.0)], 0.0, 20.0).unwrap().pop().unwrap();
    let m = measure(&b).unwrap();
    assert!(rel(m.volume, 24000.0) < 1e-9, "{}", m.volume);
    assert!(rel(m.area, 5200.0) < 1e-9);
    assert_eq!((m.faces, m.edges, m.vertices), (6, 12, 8));
    assert_eq!((m.merged.faces, m.merged.edges, m.merged.vertices), (6, 12, 8));
    assert!(m.centroid.dist(Vec3::new(20.0, 15.0, 10.0)) < 1e-6);
}

#[test]
fn cylinder_measures_match_analytic() {
    let b = extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::ZERO, 15.0), holes: vec![] }], 0.0, 50.0).unwrap().pop().unwrap();
    let m = measure(&b).unwrap();
    let v = PI * 225.0 * 50.0;
    let a = 2.0 * PI * 225.0 + 2.0 * PI * 15.0 * 50.0;
    assert!(rel(m.volume, v) < 5e-4, "{} vs {v}", m.volume);
    assert!(rel(m.area, a) < 5e-4, "{} vs {a}", m.area);
    assert_eq!((m.merged.faces, m.merged.edges, m.merged.vertices), (3, 2, 2), "{:?}", m.merged);
    assert_eq!(m.merged.face_types.get("cylinder"), Some(&1));
}

#[test]
fn extrude_on_other_planes_and_offsets() {
    let b = extrude(&Plane::XZ, &[rect(10.0, 5.0)], -2.0, 3.0).unwrap().pop().unwrap();
    let m = measure(&b).unwrap();
    assert!(rel(m.volume, 250.0) < 1e-9);
    // XZ normal is -Y: the body spans y in [-3, 2].
    assert!((m.bbox.min.y + 3.0).abs() < 1e-6 && (m.bbox.max.y - 2.0).abs() < 1e-6, "{:?}", m.bbox);
}

#[test]
fn region_with_hole() {
    let r = Region2 { outer: rect(40.0, 30.0).outer, holes: vec![Loop2::circle(Vec2::new(20.0, 15.0), 5.0).reversed()] };
    let b = extrude(&Plane::XY, &[r], 0.0, 10.0).unwrap().pop().unwrap();
    let m = measure(&b).unwrap();
    assert!(rel(m.volume, (1200.0 - PI * 25.0) * 10.0) < 5e-4, "{}", m.volume);
}

#[test]
fn box_fillet_cut_volume() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let f = fillet(&b, &[Vec3::new(0.0, 0.0, 10.0)], 3.0).unwrap();
    let mf = measure(&f).unwrap();
    let expect_f = 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0;
    assert!(rel(mf.volume, expect_f) < 1e-4, "{} vs {expect_f}", mf.volume);
    assert_eq!(mf.merged.faces, 7);
    let hole = cylinder(Vec3::new(20.0, 15.0, -5.0), Vec3::Z, 5.0, 30.0).unwrap();
    let cut = boolean(&f, &hole, BoolOp::Cut).unwrap().unwrap();
    let mc = measure(&cut).unwrap();
    let expect = expect_f - PI * 25.0 * 20.0;
    assert!(rel(mc.volume, expect) < 1e-3, "{} vs {expect}", mc.volume);
}

#[test]
fn union_and_intersect() {
    let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(5.0, 5.0, 5.0), Vec3::new(15.0, 15.0, 15.0)).unwrap();
    let u = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    assert!(rel(measure(&u).unwrap().volume, 2000.0 - 125.0) < 1e-6);
    let i = boolean(&a, &b, BoolOp::Intersect).unwrap().unwrap();
    assert!(rel(measure(&i).unwrap().volume, 125.0) < 1e-6);
}

#[test]
fn revolve_washer_and_sphere() {
    let r =
        Region2 { outer: Loop2::polygon(&[Vec2::new(10.0, 0.0), Vec2::new(15.0, 0.0), Vec2::new(15.0, 5.0), Vec2::new(10.0, 5.0)]), holes: vec![] };
    let b = revolve(&Plane::XZ, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, 2.0 * PI).unwrap().pop().unwrap();
    let m = measure(&b).unwrap();
    let v = PI * (225.0 - 100.0) * 5.0;
    assert!(rel(m.volume, v) < 5e-4, "{} vs {v}", m.volume);
    let half = revolve(&Plane::XZ, &[r], Vec2::ZERO, Vec2::Y, PI).unwrap().pop().unwrap();
    assert!(rel(measure(&half).unwrap().volume, v / 2.0) < 5e-4);
    let s = sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap();
    let ms = measure(&s).unwrap();
    assert!(rel(ms.volume, 4.0 / 3.0 * PI * 1000.0) < 1e-3, "{}", ms.volume);
}

#[test]
fn chamfer_and_step() {
    let b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
    let c = chamfer(&b, &[Vec3::new(20.0, 10.0, 20.0)], 2.0).unwrap();
    assert!(rel(measure(&c).unwrap().volume, 8000.0 - 2.0 * 20.0) < 1e-4);
    let s = step_export(&[&c], "SolveCraft").unwrap();
    assert!(s.starts_with("ISO-10303-21;"));
    assert!(s.contains("MANIFOLD_SOLID_BREP") || s.contains("CLOSED_SHELL"));
}

#[test]
fn hostile_inputs_are_errors() {
    assert!(extrude(&Plane::XY, &[rect(1.0, 1.0)], 0.0, 0.0).is_err());
    assert!(extrude(&Plane::XY, &[rect(1.0, 1.0)], 0.0, f64::NAN).is_err());
    assert!(extrude(&Plane::XY, &[], 0.0, 1.0).is_err());
    assert!(extrude(&Plane::XY, &[rect(0.0, 1.0)], 0.0, 1.0).is_err());
    assert!(box_solid(Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0)).is_err());
    assert!(cylinder(Vec3::ZERO, Vec3::ZERO, 1.0, 1.0).is_err());
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    assert!(fillet(&b, &[Vec3::new(0.0, 0.0, 5.0)], -1.0).is_err());
    assert!(fillet(&b, &[Vec3::new(500.0, 0.0, 5.0)], 1.0).is_err());
    assert!(fillet(&b, &[Vec3::new(0.0, 0.0, 5.0)], 100.0).is_err());
    let r = Region2 { outer: Loop2::polygon(&[Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)]), holes: vec![] };
    assert!(revolve(&Plane::XZ, &[r], Vec2::ZERO, Vec2::Y, 1.0).is_err(), "crosses axis");
    assert!(transform(&b, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0).is_err());
}

#[test]
fn fillets_on_several_edges_and_angles() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    // Two opposite vertical edges, then a horizontal top edge between them is not touching either.
    let f = fillet(&b, &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(40.0, 30.0, 10.0)], 4.0).unwrap();
    let corner = 16.0 - PI * 4.0;
    assert!(rel(measure(&f).unwrap().volume, 24000.0 - 2.0 * corner * 20.0) < 2e-4);
    let top = fillet(&b, &[Vec3::new(20.0, 0.0, 20.0)], 2.0).unwrap();
    assert!(rel(measure(&top).unwrap().volume, 24000.0 - (4.0 - PI) * 40.0) < 2e-4);
    // Triangular prism: 60° edges.
    let tri = Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(15.0, 25.980762113533)]), holes: vec![] };
    let p = extrude(&Plane::XY, &[tri], 0.0, 10.0).unwrap().pop().unwrap();
    let r = 2.0;
    let fp = fillet(&p, &[Vec3::new(0.0, 0.0, 5.0)], r).unwrap();
    // Removed area for interior angle φ: r²(cot(φ/2)) − r²(π − φ)/2.
    let phi = PI / 3.0;
    let removed = r * r / (phi / 2.0).tan() - r * r * (PI - phi) / 2.0;
    let v0 = measure(&p).unwrap().volume;
    assert!(rel(measure(&fp).unwrap().volume, v0 - removed * 10.0) < 2e-4, "{} vs {}", measure(&fp).unwrap().volume, v0 - removed * 10.0);
    // Adjacent edges meeting at a corner: their blends meet in a mitre.
    let two = fillet(&b, &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 15.0, 20.0)], 2.0).unwrap();
    let cut = 24000.0 - measure(&two).unwrap().volume;
    let a = 4.0 * (1.0 - PI / 4.0);
    assert!(cut > a * (20.0 + 30.0 - 2.0) && cut < a * 50.0, "{cut}");
    // Concave edge: not supported.
    let u = boolean(&b, &box_solid(Vec3::new(10.0, 10.0, 15.0), Vec3::new(20.0, 20.0, 30.0)).unwrap(), BoolOp::Union).unwrap().unwrap();
    assert!(fillet(&u, &[Vec3::new(15.0, 10.0, 20.0)], 1.0).is_err());
}

#[test]
fn tapered_extrudes() {
    // 40 x 40 square, 30 up, 10° outward: a frustum.
    let sq = Region2 {
        outer: Loop2::polygon(&[Vec2::new(-20.0, -20.0), Vec2::new(20.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)]),
        holes: vec![],
    };
    let b = extrude_tapered(&Plane::XY, &sq, 30.0, 1.0, 10f64.to_radians()).unwrap();
    let top = 40.0 + 2.0 * 30.0 * 10f64.to_radians().tan();
    let v = 30.0 / 3.0 * (1600.0 + top * top + 40.0 * top);
    let m = measure(&b).unwrap();
    assert!(rel(m.volume, v) < 1e-6, "{} vs {v}", m.volume);
    assert_eq!(m.merged.faces, 6);
    // Inward taper on a circle (a cone frustum), downward.
    let c = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] };
    let b = extrude_tapered(&Plane::XY, &c, 10.0, -1.0, -20f64.to_radians()).unwrap();
    let r2 = 10.0 - 10.0 * 20f64.to_radians().tan();
    let v = PI * 10.0 / 3.0 * (100.0 + r2 * r2 + 10.0 * r2);
    let m = measure(&b).unwrap();
    assert!(rel(m.volume, v) < 5e-4, "{} vs {v}", m.volume);
    assert!(m.bbox.max.z < 1e-6 && m.bbox.min.z > -10.0 - 1e-6, "{:?}", m.bbox);
    assert!(extrude_tapered(&Plane::XY, &c, 10.0, 1.0, -60f64.to_radians()).is_err(), "closes the profile");
}

#[test]
fn polyhedra_shell_and_draft() {
    let b = box_solid(Vec3::ZERO, Vec3::new(50.0, 40.0, 30.0)).unwrap();
    let s = shell(&b, &[Vec3::new(25.0, 20.0, 30.0)], 2.0).unwrap();
    let m = measure(&s).unwrap();
    assert!(rel(m.volume, 60000.0 - 46.0 * 36.0 * 28.0) < 1e-6, "{}", m.volume);
    assert_eq!(m.merged.faces, 11);
    // Draft two side faces 5° in, about the bottom, pulling up.
    let d = draft(&b, &[Vec3::new(0.0, 20.0, 15.0), Vec3::new(50.0, 20.0, 15.0)], &Plane::XY, Vec3::Z, 5f64.to_radians()).unwrap();
    let t = 30.0 * 5f64.to_radians().tan();
    assert!(rel(measure(&d).unwrap().volume, 40.0 * 30.0 * (50.0 + 50.0 - 2.0 * t) / 2.0) < 1e-6, "{}", measure(&d).unwrap().volume);
    let tet = convex_polyhedron(&[
        HalfSpace { n: Vec3::new(0.0, 0.0, -1.0), d: 0.0 },
        HalfSpace { n: Vec3::new(-1.0, 0.0, 0.0), d: 0.0 },
        HalfSpace { n: Vec3::new(0.0, -1.0, 0.0), d: 0.0 },
        HalfSpace { n: Vec3::new(1.0, 1.0, 1.0).normalized().unwrap(), d: 6.0 / 3f64.sqrt() },
    ])
    .unwrap();
    assert!(rel(measure(&tet).unwrap().volume, 36.0) < 1e-9);
    let cyl = cylinder(Vec3::ZERO, Vec3::Z, 5.0, 5.0).unwrap();
    let cup = shell(&cyl, &[Vec3::new(0.0, 0.0, 5.0)], 1.0).unwrap();
    assert!(rel(measure(&cup).unwrap().volume, PI * (25.0 * 5.0 - 16.0 * 4.0)) < 1e-4, "cup");
}

#[test]
fn loft_and_sweep() {
    let sq = Loop2::polygon(&[Vec2::new(-20.0, -20.0), Vec2::new(20.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)]);
    let top = Plane::XY.offset(40.0);
    let l = loft(&[(Plane::XY, sq), (top, Loop2::circle(Vec2::ZERO, 10.0))]).unwrap();
    let m = measure(&l).unwrap();
    println!("loft volume {} faces {:?}", m.volume, m.merged);
    assert!(m.volume > 314.0 * 40.0 && m.volume < 1600.0 * 40.0);
    // A tube: up 30, quarter bend of radius 20, across 30.
    let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, 3.0), holes: vec![] };
    let path = [
        PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 30.0) },
        PathSeg::Arc {
            a: Vec3::new(0.0, 0.0, 30.0),
            center: Vec3::new(20.0, 0.0, 30.0),
            axis: Vec3::new(0.0, 1.0, 0.0),
            angle: std::f64::consts::FRAC_PI_2,
        },
        PathSeg::Line { a: Vec3::new(20.0, 0.0, 50.0), b: Vec3::new(50.0, 0.0, 50.0) },
    ];
    let t = sweep(&Plane::XY, &disc, &path).unwrap();
    for tol in [0.1, 0.01, 0.003, 0.001] {
        println!("tube tess {tol}: {}", t.tessellate(tol).unwrap().measure().volume);
    }
    let area = PI * 9.0;
    let expect = area * (30.0 + 30.0 + 20.0 * std::f64::consts::FRAC_PI_2);
    assert!(rel(measure(&t).unwrap().volume, expect) < 1e-3, "{} vs {expect}", measure(&t).unwrap().volume);
}

#[test]
fn planar_booleans_with_coincident_faces() {
    // L-bracket: a wall flush with three sides of its base.
    let base = box_solid(Vec3::ZERO, Vec3::new(80.0, 50.0, 8.0)).unwrap();
    let wall = box_solid(Vec3::new(0.0, 42.0, 8.0), Vec3::new(80.0, 50.0, 50.0)).unwrap();
    let u = planar_boolean(&base, &wall, BoolOp::Union).unwrap().unwrap();
    let m = measure(&u).unwrap();
    assert!(rel(m.volume, 80.0 * 50.0 * 8.0 + 80.0 * 8.0 * 42.0) < 1e-9, "{}", m.volume);
    assert_eq!(m.merged.faces, 8, "{:?}", m.merged);
    // Through the public boolean (B-rep first, polygon fallback).
    let u2 = boolean(&base, &wall, BoolOp::Union).unwrap().unwrap();
    assert!(rel(measure(&u2).unwrap().volume, m.volume) < 1e-9);
    // Two boxes sharing a face, and a cut flush with a side.
    let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)).unwrap();
    let ab = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    assert!(rel(measure(&ab).unwrap().volume, 2000.0) < 1e-9);
    assert_eq!(measure(&ab).unwrap().merged.faces, 6);
    let notch = box_solid(Vec3::new(0.0, 0.0, 5.0), Vec3::new(5.0, 10.0, 10.0)).unwrap();
    let c = boolean(&a, &notch, BoolOp::Cut).unwrap().unwrap();
    assert!(rel(measure(&c).unwrap().volume, 1000.0 - 250.0) < 1e-9);
    let i = boolean(&a, &notch, BoolOp::Intersect).unwrap().unwrap();
    assert!(rel(measure(&i).unwrap().volume, 250.0) < 1e-9);
}

#[test]
fn sphere_booleans() {
    let s = sphere(Vec3::ZERO, 15.0).unwrap();
    let m = measure(&s).unwrap();
    assert!(rel(m.volume, 4.0 / 3.0 * PI * 3375.0) < 1e-3, "{}", m.volume);
    assert_eq!((m.merged.faces, m.merged.edges), (1, 0));
    // A ball cut out of the top face of a slab (oracle 14).
    let b = box_solid(Vec3::new(-25.0, -25.0, -20.0), Vec3::new(25.0, 25.0, 0.0)).unwrap();
    let c = boolean(&b, &s, BoolOp::Cut).unwrap().unwrap();
    let m = measure(&c).unwrap();
    assert!(rel(m.volume, 50.0 * 50.0 * 20.0 - 2.0 / 3.0 * PI * 3375.0) < 1e-3, "{}", m.volume);
    assert_eq!((m.merged.faces, m.merged.edges, m.merged.vertices), (7, 13, 9));
}

#[test]
#[ignore]
fn debug_convergence() {
    let b = extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::ZERO, 15.0), holes: vec![] }], 0.0, 50.0).unwrap().pop().unwrap();
    for t in [0.1, 0.05, 0.02, 0.01, 0.005, 0.002] {
        let m = b.tessellate(t).unwrap();
        let mm = m.measure();
        println!("tol {t}: tris {} vol {} err {:.3e}", m.triangles.len(), mm.volume, mm.volume / (PI * 225.0 * 50.0) - 1.0);
    }
    let m = b.tessellate(0.05).unwrap();
    for f in 0..4u32 {
        let ns: Vec<Vec3> =
            m.triangles.iter().zip(&m.tri_face).filter(|(_, ff)| **ff == f).flat_map(|(t, _)| t.iter().map(|k| m.normals[*k as usize])).collect();
        println!("face {f}: {} normals, first {:?}, max |n.z| {}", ns.len(), ns.first(), ns.iter().map(|n| n.z.abs()).fold(0.0, f64::max));
    }
}

#[test]
#[ignore]
fn debug_coplanar() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    for (z0, z1) in [(0.0, 20.0), (-5.0, 20.0), (0.0, 25.0), (-5.0, 25.0), (5.0, 20.0), (5.0, 25.0)] {
        let c = cylinder(Vec3::new(20.0, 15.0, z0), Vec3::Z, 5.0, z1 - z0).unwrap();
        let r = boolean(&b, &c, BoolOp::Cut);
        println!("cut z {z0}..{z1}: {:?}", r.map(|o| o.map(|b| measure(&b).unwrap().volume)));
        let r = boolean(&b, &c, BoolOp::Union);
        println!("union z {z0}..{z1}: {:?}", r.map(|o| o.map(|b| measure(&b).unwrap().volume)));
    }
    let r =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 30.0), Vec2::new(0.0, 30.0)]), holes: vec![] };
    for ang in [PI / 2.0, PI, 1.5 * PI, 2.0 * PI] {
        let q = revolve(&Plane::XY, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, ang).unwrap().pop().unwrap();
        let m = measure(&q).unwrap();
        println!("revolve {ang}: vol {} expect {} faces {}", m.volume, ang / 2.0 * 1600.0 * 30.0, m.faces);
    }
}

#[test]
#[ignore]
fn debug_blind() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let t = box_solid(Vec3::new(10.0, 10.0, 5.0), Vec3::new(20.0, 20.0, 25.0)).unwrap();
    println!("box pocket: {:?}", boolean(&b, &t, BoolOp::Cut).map(|o| o.map(|b| measure(&b).unwrap().volume)));
    let t = box_solid(Vec3::new(10.0, 10.0, 5.0), Vec3::new(20.0, 20.0, 15.0)).unwrap();
    println!("box void: {:?}", boolean(&b, &t, BoolOp::Cut).map(|o| o.map(|b| measure(&b).unwrap().volume)));
    for r in [5.0, 4.3] {
        for z in [5.0, 7.3, 13.1] {
            let c = cylinder(Vec3::new(20.3, 15.1, z), Vec3::Z, r, 30.0).unwrap();
            println!("cyl r{r} z{z}: {:?}", boolean(&b, &c, BoolOp::Cut).map(|o| o.map(|b| measure(&b).unwrap().volume)));
        }
    }
    let c = cylinder(Vec3::new(20.0, 15.0, 25.0), -Vec3::Z, 5.0, 20.0).unwrap();
    println!("cyl down: {:?}", boolean(&b, &c, BoolOp::Cut).map(|o| o.map(|b| measure(&b).unwrap().volume)));
    let s = sphere(Vec3::new(20.0, 15.0, 20.0), 5.0).unwrap();
    println!("sphere: {:?}", boolean(&b, &s, BoolOp::Cut).map(|o| o.map(|b| measure(&b).unwrap().volume)));
}

#[test]
#[ignore]
fn debug_revolve_doc_case() {
    let r =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 30.0), Vec2::new(0.0, 30.0)]), holes: vec![] };
    for ang in [PI / 2.0, -PI / 2.0] {
        let q = revolve(&Plane::XY, std::slice::from_ref(&r), Vec2::ZERO, Vec2::Y, ang).unwrap().pop().unwrap();
        let m = measure(&q).unwrap();
        println!("ang {ang}: vol {} bbox {:?}", m.volume, m.bbox);
    }
    let r2 = Region2 { outer: r.outer.reversed(), holes: vec![] };
    let q = revolve(&Plane::XY, std::slice::from_ref(&r2), Vec2::ZERO, Vec2::Y, PI / 2.0).unwrap().pop().unwrap();
    println!("reversed: {}", measure(&q).unwrap().volume);
}

#[test]
#[ignore]
fn debug_coplanar_union_rate() {
    let a = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
    let b = extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::new(40.0, 20.0), 12.0), holes: vec![] }], 0.0, 40.0).unwrap().pop().unwrap();
    let mut ok = 0;
    for i in 0..20 {
        let sh = Vec3::new(0.0137 * i as f64, 0.0291 * i as f64, -0.0173 * i as f64);
        let (a2, b2) = (transform(&a, sh, Vec3::ZERO, Vec3::Z, 0.0).unwrap(), transform(&b, sh, Vec3::ZERO, Vec3::Z, 0.0).unwrap());
        for tol in [1e-3, 1e-2, 5e-2] {
            let r = guard("t", || {
                let (sa, sb) = (a2.deep_copy(), b2.deep_copy());
                Ok(truck_shapeops::or(&sa, &sb, tol))
            });
            let good = matches!(r, Ok(Some(_)));
            if good {
                ok += 1;
            }
            println!("shift {i} tol {tol}: {}", if good { "ok" } else { "fail" });
        }
    }
    println!("ok {ok}/60");
}

#[test]
fn chamfer_top_loop_mitres() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let edges = [Vec3::new(0.0, 15.0, 20.0), Vec3::new(20.0, 0.0, 20.0), Vec3::new(40.0, 15.0, 20.0), Vec3::new(20.0, 30.0, 20.0)];
    let c = chamfer(&b, &edges, 2.0).unwrap();
    let m = measure(&c).unwrap();
    assert!(rel(m.volume, 23730.666667) < 1e-6, "{}", m.volume);
    assert_eq!(m.merged.faces, 10);
}

#[test]
#[ignore]
fn debug_chamfer_cut() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    // Prism along x: triangle in the YZ plane beyond the top-front edge.
    let plane = Plane { origin: Vec3::new(-5.0, 0.0, 0.0), x: Vec3::Y, y: Vec3::Z };
    let tri = Loop2::polygon(&[Vec2::new(2.3, 20.3), Vec2::new(-0.3, 17.7), Vec2::new(10.0, 30.0)]);
    let tri = if tri.signed_area() < 0.0 { tri.reversed() } else { tri };
    let tool = extrude(&plane, &[Region2 { outer: tri, holes: vec![] }], 0.0, 50.0).unwrap().pop().unwrap();
    println!("tool vol {}", measure(&tool).unwrap().volume);
    let r = boolean(&b, &tool, BoolOp::Cut).unwrap().unwrap();
    let m = measure(&r).unwrap();
    println!("cut: {} faces {} {:?}", m.volume, m.faces, m.merged);
    for f in r.faces(0.01).unwrap() {
        println!("face {} area {:.3} c {:?} n {:?}", f.index, f.area, f.centroid, f.plane_normal);
    }
    for tol in [0.1, 0.01, 0.001] {
        println!("tess {tol}: {}", r.tessellate(tol).unwrap().measure().volume);
    }
}

#[test]
#[ignore]
fn debug_revolved_hole_tool() {
    let b = box_solid(Vec3::ZERO, Vec3::new(60.0, 40.0, 15.0)).unwrap();
    // Drill point profile in the XZ plane through x = 50: revolve about the vertical line x = 50.
    let pl = Plane { origin: Vec3::new(50.0, 20.0, 0.0), x: Vec3::X, y: Vec3::Z };
    let r = 3.0;
    let tip = r / 59f64.to_radians().tan();
    let prof =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 16.0), Vec2::new(0.0, 5.0 - tip), Vec2::new(r, 5.0), Vec2::new(r, 16.0)]), holes: vec![] };
    let tool = revolve(&pl, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
    println!("tool vol {}", measure(&tool).unwrap().volume);
    let res = boolean(&b, &tool, BoolOp::Cut);
    println!("cut: {:?}", res.map(|o| o.map(|x| measure(&x).unwrap().volume)));
    let expect = 36000.0 - std::f64::consts::PI * 9.0 * 10.0 - std::f64::consts::PI * 9.0 * tip / 3.0;
    println!("expect {expect}");
}

#[test]
#[ignore]
fn debug_sphere() {
    let s = sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap();
    let m = measure(&s).unwrap();
    println!("vol {} area {} faces {} bbox {:?}", m.volume, m.area, m.faces, m.bbox);
    for f in s.faces(0.05).unwrap() {
        println!("face {} area {:.3} c {:?}", f.index, f.area, f.centroid);
    }
    let b = box_solid(Vec3::new(-25.0, -25.0, -20.0), Vec3::new(25.0, 25.0, 0.0)).unwrap();
    for (name, c) in [("origin", Vec3::ZERO), ("up", Vec3::new(0.0, 0.0, 3.0)), ("off", Vec3::new(1.3, 2.1, 0.7))] {
        let sp = sphere(c, 15.0).unwrap();
        println!("{name} cut {:?}", boolean(&b, &sp, BoolOp::Cut).map(|o| o.map(|x| measure(&x).unwrap().volume)));
        println!("{name} and {:?}", boolean(&b, &sp, BoolOp::Intersect).map(|o| o.map(|x| measure(&x).unwrap().volume)));
    }
}

#[test]
#[ignore]
fn debug_countersink() {
    let b = box_solid(Vec3::ZERO, Vec3::new(60.0, 40.0, 15.0)).unwrap();
    for (name, pts) in [
        ("csink", vec![Vec2::new(0.0, 0.6), Vec2::new(6.6, 0.6), Vec2::new(3.0, -3.0), Vec2::new(3.0, -31.0), Vec2::new(0.0, -31.0)]),
        ("csink-thru-top", vec![Vec2::new(0.0, 0.6), Vec2::new(6.6, 0.6), Vec2::new(3.0, -3.0), Vec2::new(3.0, -16.3), Vec2::new(0.0, -16.3)]),
        (
            "cbore",
            vec![Vec2::new(0.0, 0.6), Vec2::new(7.0, 0.6), Vec2::new(7.0, -5.0), Vec2::new(4.0, -5.0), Vec2::new(4.0, -31.0), Vec2::new(0.0, -31.0)],
        ),
    ] {
        for (pn, pl) in [
            ("xz@top", Plane::new(Vec3::new(35.0, 20.0, 15.0), Vec3::X, Vec3::Z).unwrap()),
            ("yz@top", Plane::new(Vec3::new(35.0, 20.0, 15.0), Vec3::Y, Vec3::Z).unwrap()),
            ("diag", Plane::new(Vec3::new(35.0, 20.0, 15.0), Vec3::new(0.6, 0.8, 0.0), Vec3::Z).unwrap()),
        ] {
            let tool = revolve(&pl, &[Region2 { outer: Loop2::polygon(&pts), holes: vec![] }], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)
                .unwrap()
                .pop()
                .unwrap();
            println!(
                "{name} {pn}: tool {} cut {:?}",
                measure(&tool).unwrap().volume,
                boolean(&b, &tool, BoolOp::Cut).map(|o| o.map(|x| measure(&x).unwrap().volume))
            );
        }
    }
}

#[test]
#[ignore]
fn debug_cbore_seq() {
    let b = box_solid(Vec3::ZERO, Vec3::new(60.0, 40.0, 15.0)).unwrap();
    let pl = Plane::from_normal(Vec3::new(15.0, 20.0, 15.0), Vec3::Z).unwrap();
    for (s1, s2, r2top) in [(0.0, 0.0, 0.6), (0.613, 1.226, 0.6), (0.3, 0.9, 2.37)] {
        let t1 = extrude(&pl, &[Region2 { outer: Loop2::circle_from(Vec2::ZERO, 4.0, s1), holes: vec![] }], -31.0, 0.6).unwrap().pop().unwrap();
        let c1 = boolean(&b, &t1, BoolOp::Cut).unwrap().unwrap();
        let t2 = extrude(&pl, &[Region2 { outer: Loop2::circle_from(Vec2::ZERO, 7.0, s2), holes: vec![] }], -5.0, r2top).unwrap().pop().unwrap();
        println!(
            "seams {s1} {s2}: first {} second {:?}",
            measure(&c1).unwrap().volume,
            boolean(&c1, &t2, BoolOp::Cut).map(|o| o.map(|x| measure(&x).unwrap().volume))
        );
        let c2 = boolean(&b, &t2, BoolOp::Cut).unwrap().unwrap();
        println!("  swapped: {:?}", boolean(&c2, &t1, BoolOp::Cut).map(|o| o.map(|x| measure(&x).unwrap().volume)));
    }
}

#[test]
#[ignore]
fn debug_truck_sphere() {
    use truck_modeling::{self as mt, builder};
    let mk = |c: Vec3, r: f64, axis: Vec3| -> Body {
        let ax = axis.normalized().unwrap();
        let side = ax.any_perp();
        let v0 = builder::vertex(crate::body::p3(c + ax * r));
        let arc = builder::rsweep(&v0, crate::body::p3(c), crate::body::v3(side), mt::Rad(PI));
        let shell = builder::rsweep(&arc, crate::body::p3(c), crate::body::v3(ax), mt::Rad(2.0 * PI));
        Body::new(mt::Solid::new(vec![shell])).unwrap()
    };
    let b = box_solid(Vec3::new(-25.0, -25.0, -20.0), Vec3::new(25.0, 25.0, 0.0)).unwrap();
    for (name, axis) in [("z", Vec3::Z), ("x", Vec3::X), ("diag", Vec3::new(0.3, 0.5, 0.8))] {
        let sp = mk(Vec3::ZERO, 15.0, axis);
        println!("{name}: faces {} vol {}", sp.face_count(), measure(&sp).unwrap().volume);
        for op in [BoolOp::Cut, BoolOp::Intersect, BoolOp::Union] {
            let r = guard("t", || {
                let (sa, mut sb) = (b.deep_copy(), sp.deep_copy());
                let res = match op {
                    BoolOp::Cut => {
                        sb.not();
                        truck_shapeops::and(&sa, &sb, 0.01)
                    }
                    BoolOp::Intersect => truck_shapeops::and(&sa, &sb, 0.01),
                    BoolOp::Union => truck_shapeops::or(&sa, &sb, 0.01),
                };
                Ok(res.map(|s| s.face_iter().count()))
            });
            println!("  {op:?}: {r:?}");
        }
    }
}

#[test]
#[ignore]
fn debug_sphere_patches() {
    let sp = crate::build::sphere_patches(Vec3::ZERO, 15.0).unwrap();
    let m = measure(&sp).unwrap();
    println!("vol {} want {} area {} merged {:?}", m.volume, 4.0 / 3.0 * PI * 3375.0, m.area, m.merged);
    let b = box_solid(Vec3::new(-25.0, -25.0, -20.0), Vec3::new(25.0, 25.0, 0.0)).unwrap();
    for op in [BoolOp::Cut, BoolOp::Intersect, BoolOp::Union] {
        let r = boolean(&b, &sp, op).map(|o| {
            o.map(|x| {
                let m = measure(&x).unwrap();
                (m.volume, m.merged)
            })
        });
        println!("{op:?}: {r:?}");
    }
    let c = box_solid(Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
    let a = 0.3f64;
    let (ca, sa) = (a.cos(), a.sin());
    let b2 = 0.5f64;
    let (cb, sb) = (b2.cos(), b2.sin());
    // Rz(a) * Rx(b)
    let rot = [Vec3::new(ca, sa, 0.0), Vec3::new(-sa * cb, ca * cb, sb), Vec3::new(sa * sb, -ca * sb, cb)];
    for (name, s2) in [
        ("plain", crate::build::sphere_patches(Vec3::ZERO, 25.0).unwrap()),
        ("turned", crate::build::sphere_patches_rotated(Vec3::ZERO, 25.0, rot).unwrap()),
    ] {
        println!("{name} vol {}", measure(&s2).unwrap().volume);
        println!(
            "{name} cube and sphere: {:?}",
            boolean(&c, &s2, BoolOp::Intersect).map(|o| o.map(|x| {
                let m = measure(&x).unwrap();
                (m.volume, m.merged)
            }))
        );
    }
}

#[test]
#[ignore]
fn debug_sphere_cases() {
    let s = crate::build::sphere_patches(Vec3::ZERO, 25.0).unwrap();
    let cases = [
        ("one plane", Vec3::new(-100.0, -100.0, -100.0), Vec3::new(100.0, 100.0, 20.0)),
        ("two planes", Vec3::new(-100.0, -100.0, -20.0), Vec3::new(100.0, 100.0, 20.0)),
        ("three", Vec3::new(-100.0, -100.0, -20.0), Vec3::new(20.0, 100.0, 20.0)),
        ("four", Vec3::new(-100.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)),
        ("five", Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 100.0)),
        ("cube", Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)),
        ("cube big sphere-ish", Vec3::new(-24.0, -24.0, -24.0), Vec3::new(24.0, 24.0, 24.0)),
        ("cube off", Vec3::new(-20.3, -19.7, -20.1), Vec3::new(19.9, 20.2, 19.8)),
    ];
    for (name, lo, hi) in cases {
        let b = box_solid(lo, hi).unwrap();
        for op in [BoolOp::Intersect, BoolOp::Cut] {
            let r = boolean(&b, &s, op).map(|o| o.map(|x| measure(&x).unwrap().volume));
            println!("{name} {op:?}: {:?}", r.map_err(|e| e.to_string().chars().take(60).collect::<String>()));
        }
    }
}

#[test]
#[ignore]
fn debug_sphere_rotations() {
    let b = box_solid(Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
    for k in 0..8 {
        let (a, c) = (0.37 * k as f64 + 0.1, 0.61 * k as f64 + 0.2);
        let (ca, sa, cc, sc) = (a.cos(), a.sin(), c.cos(), c.sin());
        let rot = [Vec3::new(ca, sa, 0.0), Vec3::new(-sa * cc, ca * cc, sc), Vec3::new(sa * sc, -ca * sc, cc)];
        let s = crate::build::sphere_patches_rotated(Vec3::ZERO, 25.0, rot).unwrap();
        let r = guard("t", || {
            let res = truck_shapeops::and(&b.deep_copy(), &s.deep_copy(), 0.01);
            Ok(res.map(|x| x.face_iter().count()))
        });
        println!("rot {k}: {r:?}");
    }
}

#[test]
#[ignore]
fn debug_cube_sphere_measure() {
    let b = box_solid(Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
    let s = crate::build::sphere_patches(Vec3::ZERO, 25.0).unwrap();
    let r = guard("t", || Ok(truck_shapeops::and(&b.deep_copy(), &s.deep_copy(), 0.05))).unwrap().unwrap();
    let body = Body::new(r).unwrap();
    for tol in [0.5, 0.1, 0.02] {
        let m = body.tessellate(tol).unwrap();
        let mm = m.measure();
        println!("tol {tol}: vol {} area {} tris {}", mm.volume, mm.area, m.triangles.len());
    }
    for f in body.faces(0.05).unwrap() {
        println!("face {} area {:.2} c {:?} planar {:?}", f.index, f.area, f.centroid, f.plane_normal.is_some());
    }
}

#[test]
fn mesh_bodies_move_measure_and_refuse_solid_ops() {
    // Unit cube as 12 triangles with repeated corners (welded on construction).
    let c = |i: u32| Vec3::new((i & 1) as f64 * 10.0, ((i >> 1) & 1) as f64 * 10.0, ((i >> 2) & 1) as f64 * 10.0);
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut pos = Vec::new();
    let mut tris = Vec::new();
    for q in quads {
        for t in [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] {
            let n = pos.len() as u32;
            pos.extend(t.iter().map(|i| c(*i)));
            tris.push([n, n + 1, n + 2]);
        }
    }
    let b = crate::mesh_body(&pos, &tris).unwrap();
    assert!(b.is_mesh() && b.is_closed_mesh());
    let m = measure(&b).unwrap();
    assert!((m.volume - 1000.0).abs() < 1e-9 && (m.area - 600.0).abs() < 1e-9, "{m:?}");
    assert_eq!((m.faces, m.edges, m.vertices), (12, 18, 8));
    let moved = crate::transform(&b, Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.7).unwrap();
    assert!((measure(&moved).unwrap().volume - 1000.0).abs() < 1e-9);
    let mirrored = crate::transform_matrix(&b, [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]).unwrap();
    assert!((measure(&mirrored).unwrap().volume - 1000.0).abs() < 1e-9);
    let solid = box_solid(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0)).unwrap();
    assert!(fillet(&b, &[Vec3::new(5.0, 0.0, 0.0)], 1.0).unwrap_err().to_string().contains("mesh"));
    assert!(boolean(&solid, &b, BoolOp::Union).is_err());
    assert!(crate::step_export(&[&b], "t").is_err());
    assert!(crate::mesh_body(&pos, &[[0, 1, 99]]).is_err());
    assert!(crate::mesh_body(&[Vec3::new(f64::NAN, 0.0, 0.0)], &[[0, 0, 0]]).is_err());
    assert!(crate::mesh_body(&pos, &[[0, 0, 1]]).is_err());
}

#[test]
fn concave_fillet() {
    // An L made of a base and a wall (one solid), fillet the inside corner.
    let base = box_solid(Vec3::ZERO, Vec3::new(80.0, 50.0, 8.0)).unwrap();
    let wall = box_solid(Vec3::new(0.0, 42.0, 0.0), Vec3::new(80.0, 50.0, 50.0)).unwrap();
    let l = boolean(&base, &wall, BoolOp::Union).unwrap().unwrap();
    let v0 = measure(&l).unwrap().volume;
    let f = fillet(&l, &[Vec3::new(40.0, 42.0, 8.0)], 5.0).unwrap();
    let m = measure(&f).unwrap();
    assert!(rel(m.volume, v0 + (25.0 - 25.0 * PI / 4.0) * 80.0) < 1e-4, "{} vs {}", m.volume, v0 + (25.0 - 25.0 * PI / 4.0) * 80.0);
    assert_eq!(m.merged.face_types.get("cylinder"), Some(&1));
}

#[test]
#[ignore]
fn debug_pocket_corners() {
    let b = box_solid(Vec3::ZERO, Vec3::new(80.0, 60.0, 20.0)).unwrap();
    let p = box_solid(Vec3::new(15.0, 15.0, 12.0), Vec3::new(65.0, 45.0, 25.0)).unwrap();
    let mut cur = boolean(&b, &p, BoolOp::Cut).unwrap().unwrap();
    for e in [Vec3::new(65.0, 15.0, 16.0), Vec3::new(15.0, 15.0, 16.0), Vec3::new(65.0, 45.0, 16.0), Vec3::new(15.0, 45.0, 16.0)] {
        match fillet(&cur, &[e], 5.0) {
            Ok(n) => {
                println!("ok {e:?} vol {}", measure(&n).unwrap().volume);
                cur = n;
            }
            Err(err) => {
                println!("fail {e:?}: {err}");
                break;
            }
        }
    }
}

#[test]
fn pocket_floor_loop_fillet() {
    let b = box_solid(Vec3::ZERO, Vec3::new(80.0, 60.0, 20.0)).unwrap();
    let p = box_solid(Vec3::new(15.0, 15.0, 12.0), Vec3::new(65.0, 45.0, 25.0)).unwrap();
    let mut cur = boolean(&b, &p, BoolOp::Cut).unwrap().unwrap();
    cur = fillet(&cur, &[Vec3::new(65.0, 15.0, 16.0), Vec3::new(15.0, 15.0, 16.0), Vec3::new(65.0, 45.0, 16.0), Vec3::new(15.0, 45.0, 16.0)], 5.0)
        .unwrap();
    let v1 = measure(&cur).unwrap().volume;
    let floor: Vec<Vec3> = [(40.0, 15.0), (65.0, 30.0), (40.0, 45.0), (15.0, 30.0)].iter().map(|(x, y)| Vec3::new(*x, *y, 12.0)).collect();
    let mut pts = floor.clone();
    let d = 5.0 - 5.0 / 2f64.sqrt();
    pts.extend([(15.0 + d, 15.0 + d), (65.0 - d, 15.0 + d), (65.0 - d, 45.0 - d), (15.0 + d, 45.0 - d)].iter().map(|(x, y)| Vec3::new(*x, *y, 12.0)));
    let f = fillet(&cur, &pts, 2.0).unwrap();
    let m = measure(&f).unwrap();
    // Added: the fillet profile (r²(1 − π/4)) swept along the floor outline (Pappus for the
    // corners: the profile's centroid sits 2 − 4·2/(3(4 − π))·… inward; check loosely).
    let straight = 2.0 * (40.0 + 20.0);
    let added_lines = (4.0 - PI) * straight;
    assert!(m.volume > v1 + added_lines && m.volume < v1 + added_lines + 4.0 * 2.0 * PI * 5.0, "{} {}", m.volume, v1);
    println!("pocket: {:?} vol {}", m.merged, m.volume);
}

#[test]
fn round_every_edge_of_a_box() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let mut pts = Vec::new();
    for e in b.edges(0.1).unwrap() {
        pts.push(e.mid);
    }
    let f = fillet(&b, &pts, 3.0).unwrap();
    let m = measure(&f).unwrap();
    let r = 3.0f64;
    let (a, bb, c) = (40.0 - 2.0 * r, 30.0 - 2.0 * r, 20.0 - 2.0 * r);
    // Inner box grown by a ball (Steiner): V = abc + 2r(ab+bc+ca) + πr²(a+b+c) + 4/3πr³.
    let want = a * bb * c + 2.0 * r * (a * bb + bb * c + c * a) + PI * r * r * (a + bb + c) + 4.0 / 3.0 * PI * r.powi(3);
    assert!(rel(m.volume, want) < 1e-3, "{} vs {want}", m.volume);
    assert_eq!((m.merged.faces, m.merged.edges, m.merged.vertices), (26, 48, 24), "{:?}", m.merged);
}

#[test]
#[ignore]
fn debug_heal_sphere_cut() {
    let b = box_solid(Vec3::new(-25.0, -25.0, -20.0), Vec3::new(25.0, 25.0, 0.0)).unwrap();
    let s = sphere(Vec3::ZERO, 15.0).unwrap();
    let r = guard("t", || {
        let mut sb = s.deep_copy();
        sb.not();
        Ok(truck_shapeops::and(&b.deep_copy(), &sb, 0.05))
    })
    .unwrap()
    .unwrap();
    let raw = Body::new(r.clone()).unwrap();
    let healed = Body::new(crate::heal::heal(r, 60.0)).unwrap();
    for (name, x) in [("raw", raw), ("healed", healed)] {
        println!("{name}: vol {} faces {}", measure(&x).unwrap().volume, x.face_count());
        for f in x.faces(0.05).unwrap() {
            println!("  face {} area {:.2} c {:?}", f.index, f.area, f.centroid);
        }
    }
}

#[test]
#[ignore]
fn debug_coplanar_cylinder_union() {
    let a = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
    let c = cylinder(Vec3::new(40.0, 20.0, 0.0), Vec3::Z, 12.0, 40.0).unwrap();
    for op in [BoolOp::Union, BoolOp::Cut, BoolOp::Intersect] {
        let r = guard("t", || {
            let (sa, mut sb) = (a.deep_copy(), c.deep_copy());
            Ok(match op {
                BoolOp::Union => truck_shapeops::or(&sa, &sb, 0.05),
                BoolOp::Cut => {
                    sb.not();
                    truck_shapeops::and(&sa, &sb, 0.05)
                }
                BoolOp::Intersect => truck_shapeops::and(&sa, &sb, 0.05),
            }
            .map(|s| s.face_iter().count()))
        });
        println!("{op:?}: {r:?}");
    }
}

#[test]
fn coplanar_curved_booleans() {
    // A box and a cylinder standing on the same plane, the cylinder's axis on the box's side.
    let a = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
    let c = cylinder(Vec3::new(40.0, 20.0, 0.0), Vec3::Z, 12.0, 40.0).unwrap();
    let half = PI * 144.0 / 2.0;
    let u = boolean(&a, &c, BoolOp::Union).unwrap().unwrap();
    let m = measure(&u).unwrap();
    assert!(rel(m.volume, 32000.0 + half * 40.0 + half * 20.0) < 1e-3, "{}", m.volume);
    let cut = boolean(&a, &c, BoolOp::Cut).unwrap().unwrap();
    assert!(rel(measure(&cut).unwrap().volume, 32000.0 - half * 20.0) < 1e-3);
    let i = boolean(&a, &c, BoolOp::Intersect).unwrap().unwrap();
    assert!(rel(measure(&i).unwrap().volume, half * 20.0) < 1e-3);
    println!("union {:?}", m.merged);
}

#[test]
fn shell_non_convex() {
    // An L-shaped block opened at the top of its tall part.
    let base = box_solid(Vec3::ZERO, Vec3::new(60.0, 40.0, 10.0)).unwrap();
    let tower = box_solid(Vec3::ZERO, Vec3::new(20.0, 40.0, 40.0)).unwrap();
    let l = boolean(&base, &tower, BoolOp::Union).unwrap().unwrap();
    let s = shell(&l, &[Vec3::new(10.0, 20.0, 40.0)], 2.0).unwrap();
    // Outer volume minus the cavity (the L shrunk by 2, open upward).
    let outer = 60.0 * 40.0 * 10.0 + 20.0 * 40.0 * 30.0;
    let cavity = 56.0 * 36.0 * 6.0 + 16.0 * 36.0 * 32.0;
    let v = measure(&s).unwrap().volume;
    assert!(rel(v, outer - cavity) < 1e-6, "{v} vs {}", outer - cavity);
}

#[test]
#[ignore]
fn debug_drilled_hole() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 15.0)).unwrap();
    let r = 3.4;
    let half = (118f64 / 2.0).to_radians();
    let tip = r / half.tan();
    let (top, bottom) = (0.6, -12.0);
    let a = std::env::var("ANG").ok().and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
    let rp = Plane::new(Vec3::new(20.0, 15.0, 15.0), Vec3::new(a.cos(), a.sin(), 0.0), Vec3::Z).unwrap();
    let tipv = if std::env::var_os("NOTIP").is_some() { 0.0 } else { tip };
    let pts = [Vec2::new(0.0, top), Vec2::new(r, top), Vec2::new(r, bottom), Vec2::new(0.0, bottom - tipv)];
    let region = Region2 { outer: Loop2::polygon(&pts), holes: vec![] };
    let tool = revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
    println!("tool faces {} vol {}", tool.face_count(), measure(&tool).unwrap().volume);
    let r = guard("t", || {
        let mut sb = tool.deep_copy();
        sb.not();
        Ok(truck_shapeops::and(&b.deep_copy(), &sb, 0.02).map(|s| s.face_iter().count()))
    });
    println!("cut: {r:?}");
}

#[test]
#[ignore]
fn debug_face_param_areas() {
    use truck_modeling::{BoundedCurve, ParametricCurve, SearchNearestParameter};
    let rp = Plane::new(Vec3::new(20.0, 15.0, 15.0), Vec3::X, Vec3::Z).unwrap();
    let pts = [Vec2::new(0.0, 0.6), Vec2::new(3.4, 0.6), Vec2::new(3.4, -12.0), Vec2::new(0.0, -12.0)];
    let region = Region2 { outer: Loop2::polygon(&pts), holes: vec![] };
    let tool = revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
    let ex = cylinder(Vec3::new(20.0, 15.0, 3.0), Vec3::Z, 3.4, 12.6).unwrap();
    for (name, b) in [("revolve", &tool), ("extrude", &ex)] {
        for f in b.solid.face_iter() {
            let s = f.surface();
            let mut poly: Vec<(f64, f64)> = Vec::new();
            for w in f.absolute_boundaries() {
                for e in w.edge_iter() {
                    let c = e.oriented_curve();
                    let (t0, t1) = c.range_tuple();
                    for k in 0..8 {
                        let p = c.subs(t0 + (t1 - t0) * k as f64 / 8.0);
                        if let Some(uv) = s.search_nearest_parameter(p, poly.last().copied(), 100) {
                            poly.push(uv);
                        }
                    }
                }
            }
            let n = poly.len();
            let area: f64 = (0..n)
                .map(|i| {
                    let (a, b) = (poly[i], poly[(i + 1) % n]);
                    a.0 * b.1 - b.0 * a.1
                })
                .sum::<f64>()
                / 2.0;
            println!("{name}: orientation {} param area {area:.4} wires {}", f.orientation(), f.absolute_boundaries().len());
        }
    }
}

#[test]
#[ignore]
fn debug_revolved_tube_cut() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 15.0)).unwrap();
    let rp = Plane::new(Vec3::new(20.0, 15.0, 15.0), Vec3::X, Vec3::Z).unwrap();
    for (name, inner) in [("tube", 1.0), ("solid", 0.0)] {
        let pts = [Vec2::new(inner, 0.6), Vec2::new(3.4, 0.6), Vec2::new(3.4, -12.0), Vec2::new(inner, -12.0)];
        let region = Region2 { outer: Loop2::polygon(&pts), holes: vec![] };
        let tool = revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
        let r = guard("t", || {
            let mut sb = tool.deep_copy();
            sb.not();
            Ok(truck_shapeops::and(&b.deep_copy(), &sb, 0.02).map(|s| s.face_iter().count()))
        });
        println!("{name}: faces {} cut {r:?}", tool.face_count());
    }
}

#[test]
fn circle_loop_fillets_and_chamfers() {
    // A boss on a plate: concave fillet where it meets the plate, convex at its top.
    let plate = box_solid(Vec3::new(-30.0, -30.0, 0.0), Vec3::new(30.0, 30.0, 8.0)).unwrap();
    let boss = cylinder(Vec3::new(0.0, 0.0, 8.0), Vec3::Z, 10.0, 15.0).unwrap();
    let b = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
    let v0 = measure(&b).unwrap().volume;
    let f = fillet(&b, &[Vec3::new(-10.0, 0.0, 8.0)], 3.0).unwrap();
    let v1 = measure(&f).unwrap().volume;
    // Concave: the profile area r²(1 − π/4) swept round a circle of radius ≈ 10 + r/3.
    let added = 9.0 * (1.0 - PI / 4.0) * 2.0 * PI * (10.0 + 3.0 * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI));
    assert!((v1 - v0 - added).abs() < 0.02 * added, "{} {}", v1 - v0, added);
    let c = chamfer(&f, &[Vec3::new(-10.0, 0.0, 23.0)], 1.0).unwrap();
    let v2 = measure(&c).unwrap().volume;
    // Convex chamfer: a 1×1 triangle round a circle of radius ≈ 10 − 1/3.
    let removed = 0.5 * 2.0 * PI * (10.0 - 1.0 / 3.0);
    assert!((v1 - v2 - removed).abs() < 0.02 * removed, "{} {}", v1 - v2, removed);
}

#[test]
fn trimmed_revolved_faces_measure_exactly() {
    use solvecraft_geom::Seg2;
    use std::f64::consts::FRAC_PI_2;
    // A slot with concave ends sketched on the plane x = 12.5, cut 4 deep into a revolved shaft
    // of radius 12.5 (the cut faces' triangulation spans wide angles unless refined).
    let segs = vec![
        Seg2::Arc { center: Vec2::new(-45.0, 0.0), radius: 4.0, start: FRAC_PI_2, sweep: PI },
        Seg2::Line { a: Vec2::new(-45.0, -4.0), b: Vec2::new(-75.0, -4.0) },
        Seg2::Arc { center: Vec2::new(-75.0, 0.0), radius: 4.0, start: -FRAC_PI_2, sweep: PI },
        Seg2::Line { a: Vec2::new(-75.0, 4.0), b: Vec2::new(-45.0, 4.0) },
    ];
    let pl = Plane::new(Vec3::new(12.5, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0), Vec3::Y).unwrap();
    let tool = extrude(&pl, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], -4.0, 1.0).unwrap().pop().unwrap();
    let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, -30.0), (12.5, -30.0), (12.5, -90.0), (10.0, -90.0), (10.0, -120.0), (0.0, -120.0)];
    let segs = (0..8).map(|i| Seg2::Line { a: Vec2::new(pts[i].0, pts[i].1), b: Vec2::new(pts[(i + 1) % 8].0, pts[(i + 1) % 8].1) }).collect();
    let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, -1.0)).unwrap();
    let shaft = revolve(&xz, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], Vec2::ZERO, Vec2::new(0.0, -1.0), 2.0 * PI).unwrap().pop().unwrap();
    let c = boolean(&shaft, &tool, BoolOp::Cut).unwrap().unwrap();
    // Removed: ∫∫ over the slot of (√(12.5² − y²) − 8.5), by a fine midpoint rule.
    let (n, mut want) = (800, 0.0);
    for i in 0..n {
        let z = 30.0 + 60.0 * (i as f64 + 0.5) / n as f64;
        for j in 0..n {
            let y = -4.0 + 8.0 * (j as f64 + 0.5) / n as f64;
            if (45.0..=75.0).contains(&z) && (z - 45.0).powi(2) + y * y >= 16.0 && (z - 75.0).powi(2) + y * y >= 16.0 {
                want += ((156.25 - y * y).sqrt() - 8.5) * (60.0 / n as f64) * (8.0 / n as f64);
            }
        }
    }
    let got = measure(&shaft).unwrap().volume - measure(&c).unwrap().volume;
    assert!((got - want).abs() < 0.01 * want, "removed {got}, want {want}");
}

#[test]
fn shell_with_cylindrical_faces() {
    // A box with its four vertical edges rounded, opened at the top.
    let b = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
    let edges: Vec<Vec3> = [(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)].iter().map(|(x, y)| Vec3::new(*x, *y, 15.0)).collect();
    let r = fillet(&b, &edges, 6.0).unwrap();
    let s = shell(&r, &[Vec3::new(50.0, 30.0, 30.0)], 2.0).unwrap();
    let outer = (100.0 * 60.0 - (4.0 - PI) * 36.0) * 30.0;
    let cavity = (96.0 * 56.0 - (4.0 - PI) * 16.0) * 28.0;
    let v = measure(&s).unwrap().volume;
    assert!(rel(v, outer - cavity) < 1e-4, "{v} vs {}", outer - cavity);
    // A plate with a round boss, opened underneath: the cavity follows the boss.
    let plate = box_solid(Vec3::new(-30.0, -30.0, 0.0), Vec3::new(30.0, 30.0, 10.0)).unwrap();
    let boss = cylinder(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 12.0, 25.0).unwrap();
    let pb = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
    let s = shell(&pb, &[Vec3::new(0.0, 0.0, 0.0)], 2.0).unwrap();
    let outer = 3600.0 * 10.0 + PI * 144.0 * 20.0;
    let cavity = 56.0 * 56.0 * 8.0 + PI * 100.0 * 20.0;
    let v = measure(&s).unwrap().volume;
    assert!(rel(v, outer - cavity) < 1e-4, "{v} vs {}", outer - cavity);
}

#[test]
fn draft_of_a_round_ended_pocket() {
    use solvecraft_geom::Seg2;
    use std::f64::consts::FRAC_PI_2;
    let block = box_solid(Vec3::ZERO, Vec3::new(80.0, 60.0, 25.0)).unwrap();
    let segs = vec![
        Seg2::Line { a: Vec2::new(25.0, 20.0), b: Vec2::new(55.0, 20.0) },
        Seg2::Arc { center: Vec2::new(55.0, 30.0), radius: 10.0, start: -FRAC_PI_2, sweep: PI },
        Seg2::Line { a: Vec2::new(55.0, 40.0), b: Vec2::new(25.0, 40.0) },
        Seg2::Arc { center: Vec2::new(25.0, 30.0), radius: 10.0, start: FRAC_PI_2, sweep: PI },
    ];
    let tool = extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 15.0, 30.0).unwrap().pop().unwrap();
    let b = boolean(&block, &tool, BoolOp::Cut).unwrap().unwrap();
    let walls = [Vec3::new(15.0, 30.0, 20.0), Vec3::new(40.0, 20.0, 20.0), Vec3::new(65.0, 30.0, 20.0), Vec3::new(40.0, 40.0, 20.0)];
    let a = 5f64.to_radians();
    let d = draft(&b, &walls, &Plane::XY.offset(25.0), Vec3::Z, a).unwrap();
    // The pocket narrows going down: at depth t its walls stand t·tan(a) further in.
    let (n, mut pocket) = (2000, 0.0);
    for k in 0..n {
        let e = (k as f64 + 0.5) / n as f64 * 10.0 * a.tan();
        pocket += (30.0 * (20.0 - 2.0 * e) + PI * (10.0 - e).powi(2)) * 10.0 / n as f64;
    }
    let v = measure(&d).unwrap().volume;
    assert!(rel(v, 80.0 * 60.0 * 25.0 - pocket) < 1e-4, "{v} vs {}", 80.0 * 60.0 * 25.0 - pocket);
}

#[test]
fn rounded_corners_rounded_again_make_spheres() {
    // Vertical edges first, then the top and bottom loops with the same radius: the corners
    // become sphere octants, as if every edge were rounded at once.
    let (a, b, c, r) = (40.0, 30.0, 20.0, 2.0);
    let bx = box_solid(Vec3::ZERO, Vec3::new(a, b, c)).unwrap();
    let vert: Vec<Vec3> = [(0.0, 0.0), (a, 0.0), (a, b), (0.0, b)].iter().map(|(x, y)| Vec3::new(*x, *y, c / 2.0)).collect();
    let f = fillet(&bx, &vert, r).unwrap();
    let f = fillet(&f, &[Vec3::new(a / 2.0, 0.0, c)], r).unwrap();
    let f = fillet(&f, &[Vec3::new(a / 2.0, 0.0, 0.0)], r).unwrap();
    let (ai, bi, ci) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    let area = 2.0 * (ai * bi + bi * ci + ai * ci) + PI * r * (ai + bi + ci) * 2.0 + 4.0 * PI * r * r;
    let vol = ai * bi * ci + 2.0 * r * (ai * bi + bi * ci + ai * ci) + PI * r * r * (ai + bi + ci) + 4.0 / 3.0 * PI * r * r * r;
    let m = measure(&f).unwrap();
    println!("area {} want {area}; volume {} want {vol}; faces {:?}", m.area, m.volume, m.merged);
    assert!(rel(m.volume, vol) < 1e-4, "volume {} vs {vol}", m.volume);
    assert!(rel(m.area, area) < 1e-4, "area {} vs {area}", m.area);
}

#[test]
fn every_edge_of_an_l_block() {
    // An L (60 × 40, legs 8 wide) 20 thick with all 18 edges rounded by 2: the inner vertical
    // edge is concave (its corners become tori), the others convex (sphere corners).
    let pts = [(0.0, 0.0), (60.0, 0.0), (60.0, 8.0), (8.0, 8.0), (8.0, 40.0), (0.0, 40.0)];
    let outer = Loop2::polygon(&pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect::<Vec<_>>());
    let l = extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
    let edges: Vec<Vec3> = l.edges(0.1).unwrap().iter().map(|e| e.mid).collect();
    let f = fillet(&l, &edges, 2.0).unwrap();
    let m = measure(&f).unwrap();
    // Area by pieces: walls (lengths less the roundings, 16 high), top and bottom (the L shrunk
    // by 2 with a radius-4 inner corner), quarter cylinders along every straight edge, ten
    // sphere octants, and two torus quarters (Pappus: arc π, centroid radius 4 − 4/π, angle π/2).
    let walls = 16.0 * (56.0 + 4.0 + 48.0 + 28.0 + 4.0 + 36.0);
    let caps = 2.0 * (56.0 * 4.0 + 4.0 * 32.0 + 16.0 - 4.0 * PI);
    let vertical = 6.0 * PI * 16.0;
    let horizontal = 2.0 * PI * (56.0 + 4.0 + 48.0 + 28.0 + 4.0 + 36.0);
    let spheres = 10.0 * 2.0 * PI;
    let tori = 2.0 * PI * (4.0 - 4.0 / PI) * PI / 2.0;
    let area = walls + caps + vertical + horizontal + spheres + tori;
    assert!(rel(m.area, area) < 1e-4, "area {} vs {area}", m.area);
    assert_eq!((m.merged.faces, m.merged.edges, m.merged.vertices), (38, 74, 38));
}

#[test]
fn overlapping_holes_and_mirror_joins() {
    // Overlapping through holes in a plate: done on the plate's section, exactly.
    let plate = box_solid(Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 5.0)).unwrap();
    let hole = |x: f64, y: f64| {
        let c = Loop2::circle(Vec2::new(x, y), 5.0);
        extrude(&Plane::XY, &[Region2 { outer: c, holes: vec![] }], -1.0, 6.0).unwrap().pop().unwrap()
    };
    let mut cur = plate;
    for (x, y) in [(30.0, 0.0), (25.0, 0.0)] {
        cur = boolean(&cur, &hole(x, y), BoolOp::Cut).unwrap().unwrap();
    }
    // Two discs of radius 5, centres 5 apart: their union's area is 2·25π − (50π/3 − 25√3/2).
    let lens = 50.0 * PI / 3.0 - 25.0 * 3f64.sqrt() / 2.0;
    let want = 50000.0 - (50.0 * PI - lens) * 5.0;
    assert!(rel(measure(&cur).unwrap().volume, want) < 1e-5, "{} vs {want}", measure(&cur).unwrap().volume);
    // A bracket with rounded corners joined with its mirror image across the face they share.
    let half = box_solid(Vec3::ZERO, Vec3::new(30.0, 40.0, 10.0)).unwrap();
    let half = fillet(&half, &[Vec3::new(30.0, 0.0, 5.0), Vec3::new(30.0, 40.0, 5.0)], 5.0).unwrap();
    let m = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let other = transform_matrix(&half, m).unwrap();
    let both = boolean(&half, &other, BoolOp::Union).unwrap().unwrap();
    let v = measure(&half).unwrap().volume;
    let mb = measure(&both).unwrap();
    assert!(rel(mb.volume, 2.0 * v) < 1e-9, "{} vs {}", mb.volume, 2.0 * v);
    // One body: the shared face is gone and the faces either side of it merged.
    assert_eq!(mb.merged.faces, 10, "{:?}", mb.merged);
    // Two equal cylinders on one axis, overlapping: one longer cylinder.
    let c1 = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 30.0).unwrap();
    let c2 = cylinder(Vec3::new(0.0, 0.0, 20.0), Vec3::Z, 10.0, 30.0).unwrap();
    let u = measure(&boolean(&c1, &c2, BoolOp::Union).unwrap().unwrap()).unwrap();
    let v1 = measure(&c1).unwrap().volume;
    assert!(rel(u.volume, v1 * 5.0 / 3.0) < 2e-4, "{} (one: {v1})", u.volume);
    assert_eq!(u.merged.faces, 3, "{:?}", u.merged);
}

#[test]
fn torus_cut_through_its_middle_plane() {
    // A torus whose tube centre lies in a box's top face: the box's top cuts the torus where
    // the tube is vertical. (The revolve profile's seam points used to lie in that plane.)
    let bx = box_solid(Vec3::new(-30.0, -30.0, -10.0), Vec3::new(30.0, 30.0, 0.0)).unwrap();
    let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, -1.0)).unwrap();
    let ring = Region2 { outer: Loop2::circle(Vec2::new(22.0, 0.0), 4.0), holes: vec![] };
    let torus = revolve(&xz, &[ring], Vec2::ZERO, Vec2::new(0.0, -1.0), 2.0 * PI).unwrap().pop().unwrap();
    let c = boolean(&bx, &torus, BoolOp::Cut).unwrap().unwrap();
    // Half the torus volume (2π²Rr²) is removed.
    let want = 36000.0 - PI * PI * 22.0 * 16.0;
    let v = measure(&c).unwrap().volume;
    assert!(rel(v, want) < 1e-4, "{v} vs {want}");
    // A revolved tube cut across: both walls cross the box's top face (nested loops on it).
    let tube =
        Region2 { outer: Loop2::polygon(&[Vec2::new(5.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, -30.0), Vec2::new(5.0, -30.0)]), holes: vec![] };
    let t = revolve(&xz, &[tube], Vec2::ZERO, Vec2::new(0.0, -1.0), 2.0 * PI).unwrap().pop().unwrap();
    let bx = box_solid(Vec3::new(-40.0, -40.0, -40.0), Vec3::new(40.0, 40.0, 10.0)).unwrap();
    let i = boolean(&t, &bx, BoolOp::Intersect).unwrap().unwrap();
    let v = measure(&i).unwrap().volume;
    assert!(rel(v, PI * 75.0 * 10.0) < 1e-4, "{v}");
}

#[test]
fn fillet_where_a_branch_pipe_meets_a_main_pipe() {
    // A branch (radius 10) on a main pipe (radius 15): the saddle curve between the two
    // cylinders, blended by rolling a ball of radius 3 along it.
    let main = cylinder(Vec3::new(-40.0, 0.0, 0.0), Vec3::X, 15.0, 80.0).unwrap();
    let branch = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 35.0).unwrap();
    let tee = boolean(&main, &branch, BoolOp::Union).unwrap().unwrap();
    let v0 = measure(&tee).unwrap().volume;
    let f = fillet(&tee, &[Vec3::new(0.0, 10.0, 125f64.sqrt())], 3.0).unwrap();
    let m = measure(&f).unwrap();
    // The added material: less than r²(1 − π/4) (a right-angled corner's) swept round the
    // ≈ 65 mm saddle, as the pipes meet at obtuse angles, but well over a third of it.
    let bare = 9.0 * (1.0 - PI / 4.0) * 65.0;
    let added = m.volume - v0;
    assert!(added > 0.35 * bare && added < bare, "added {added} (bare {bare})");
    // Main cylinder, branch cylinder, the blend, the branch's top and the main's two ends.
    assert_eq!(m.merged.faces, 6, "{:?}", m.merged);
}

#[test]
fn fillet_an_open_curved_edge() {
    use solvecraft_geom::Seg2;
    // A D-shaped block: the round top edge only, ending at the flat wall's sharp corners.
    let d = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) },
        ],
    };
    let b = extrude(&Plane::XY, &[Region2 { outer: d, holes: vec![] }], 0.0, 15.0).unwrap().pop().unwrap();
    let v0 = measure(&b).unwrap().volume;
    let f = fillet(&b, &[Vec3::new(0.0, 10.0, 15.0)], 2.0).unwrap();
    // (A fine tessellation: the difference of two measured volumes magnifies their error.)
    let _ = v0;
    let fine = |x: &Body| x.tessellate_with(0.002, true).unwrap().measure().volume;
    let removed = fine(&b) - fine(&f);
    // Pappus: the corner's cross-section r²(1 − π/4), its centroid 0.2234 r in from the wall.
    let r = 2.0;
    let want = r * r * (1.0 - PI / 4.0) * (10.0 - r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI))) * PI;
    assert!(rel(removed, want) < 1e-3, "{removed} vs {want}");
}

/// Boolean survey: random boxes, cylinders and spheres combined every way; prints what fails.
#[test]
#[ignore]
fn boolean_survey() {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 1_000_000) as f64 / 1_000_000.0
    };
    let mut shapes: Vec<(String, Body)> = Vec::new();
    for i in 0..40 {
        let kind = i % 4;
        let (x, y, z) = ((rnd() - 0.5) * 20.0, (rnd() - 0.5) * 20.0, (rnd() - 0.5) * 20.0);
        // Snap some coordinates to a 5 mm grid: coincident faces and tangencies.
        let snap = |v: f64, s: bool| if s { (v / 5.0).round() * 5.0 } else { v };
        let s = rnd() < 0.5;
        let (x, y, z) = (snap(x, s), snap(y, s), snap(z, s));
        let a = 5.0 + snap(rnd() * 20.0, s);
        let ax = (rnd() * 3.0) as usize % 3;
        let b = match kind {
            0 => box_solid(Vec3::new(x, y, z), Vec3::new(x + a, y + a * 0.8, z + a * 0.6)),
            1 => cylinder(Vec3::new(x, y, z), [Vec3::X, Vec3::Y, Vec3::Z][ax], a * 0.4, a),
            2 => sphere(Vec3::new(x, y, z), a * 0.5),
            _ => box_solid(Vec3::new(x, y, z), Vec3::new(x + a * 0.5, y + a, z + a * 0.3)),
        };
        if let Ok(b) = b {
            shapes.push((format!("{}{i}@({x:.3},{y:.3},{z:.3}) a={a:.3} ax={ax}", ["box", "cyl", "sph", "slab"][kind]), b));
        }
    }
    let (mut ok, mut empty, mut fail) = (0, 0, Vec::new());
    for i in 0..shapes.len() {
        for j in i + 1..(i + 4).min(shapes.len()) {
            for op in [BoolOp::Union, BoolOp::Cut, BoolOp::Intersect] {
                let (na, a) = &shapes[i];
                let (nb, b) = &shapes[j];
                match boolean(a, b, op) {
                    Ok(Some(_)) => ok += 1,
                    Ok(None) => empty += 1,
                    Err(e) => fail.push(format!("{na} {op:?} {nb}: {}", e.to_string().chars().take(140).collect::<String>())),
                }
            }
        }
    }
    println!("ok {ok} empty {empty} failed {}", fail.len());
    for f in &fail {
        println!("  {f}");
    }
}

#[test]
fn booleans_found_by_the_survey() {
    // A tool wholly inside: a void (the solid keeps an inner shell).
    let sp = sphere(Vec3::new(-2.894, 7.008, 8.081), 8.869).unwrap();
    let slab = box_solid(Vec3::new(-5.0, 0.0, 5.0), Vec3::new(-2.5, 5.0, 6.5)).unwrap();
    let c = boolean(&sp, &slab, BoolOp::Cut).unwrap().unwrap();
    let want = 4.0 / 3.0 * PI * 8.869f64.powi(3) - 18.75;
    assert!(rel(measure(&c).unwrap().volume, want) < 1e-3, "{}", measure(&c).unwrap().volume);
    // A sphere clipping a box's corner: the result comes out outward-facing.
    let sp = sphere(Vec3::new(-8.226, 9.539, -1.251), 11.8435).unwrap();
    let bx = box_solid(Vec3::new(0.0, -5.0, -5.0), Vec3::new(15.0, 7.0, 4.0)).unwrap();
    let c = boolean(&bx, &sp, BoolOp::Cut).unwrap().unwrap();
    let v = measure(&c).unwrap().volume;
    assert!(v > 1400.0 && v < 1620.0, "{v}");
    // A cylinder and a box sharing an end plane, the box's side near-tangent to the cylinder.
    let cy = cylinder(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, 8.0, 20.0).unwrap();
    let bx = box_solid(Vec3::new(0.0, -5.0, -5.0), Vec3::new(15.0, 7.0, 4.0)).unwrap();
    let u = boolean(&cy, &bx, BoolOp::Union).unwrap().unwrap();
    let vu = measure(&u).unwrap().volume;
    assert!(vu > PI * 64.0 * 20.0 && vu < PI * 64.0 * 20.0 + 1620.0, "{vu}");
}

#[test]
fn fillet_two_edges_meeting_at_a_corner() {
    // Two top edges of a box meeting at a corner (the vertical edge there stays sharp): the
    // fillets meet in a mitre and stop square at the far walls.
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let f = fillet(&b, &[Vec3::new(20.0, 0.0, 20.0), Vec3::new(0.0, 15.0, 20.0)], 3.0).unwrap();
    let removed = 24000.0 - measure(&f).unwrap().volume;
    // Each edge loses r²(1 − π/4) along its length; at the mitred corner the two cuts overlap
    // in a region of volume r³(1 − π/4)·… — bounded between the sum less one corner and the sum.
    let a = 9.0 * (1.0 - PI / 4.0);
    let (lo, hi) = (a * (40.0 + 30.0) - a * 3.0, a * (40.0 + 30.0));
    assert!(removed > lo && removed < hi, "removed {removed} in ({lo}, {hi})");
    // Three edges: front, left and back.
    let f3 = fillet(&b, &[Vec3::new(20.0, 0.0, 20.0), Vec3::new(0.0, 15.0, 20.0), Vec3::new(20.0, 30.0, 20.0)], 3.0).unwrap();
    assert!(measure(&f3).unwrap().volume < measure(&f).unwrap().volume);
}

#[test]
fn fillet_three_edges_at_a_corner() {
    // The three edges meeting at one corner of a box: the corner becomes a sphere octant.
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let r = 3.0;
    let f = fillet(&b, &[Vec3::new(20.0, 0.0, 20.0), Vec3::new(0.0, 15.0, 20.0), Vec3::new(0.0, 0.0, 10.0)], r).unwrap();
    let removed = 24000.0 - measure(&f).unwrap().volume;
    // Three edges lose r²(1 − π/4) each along their length; the corner, a cube r³ less an
    // octant of the ball, was counted three times over and is removed once (r³ − πr³/6).
    let a = r * r * (1.0 - PI / 4.0);
    let want = a * (40.0 + 30.0 + 20.0) - 3.0 * a * r + (r * r * r - PI * r * r * r / 6.0);
    assert!(rel(removed, want) < 2e-3, "removed {removed} vs {want}");
    let m = measure(&f).unwrap().merged;
    assert_eq!(m.face_types.get("sphere"), Some(&1), "{m:?}");
}

#[test]
fn shell_with_spherical_and_conical_faces() {
    // A cylinder capped by a hemisphere (a dome), revolved, opened at the bottom.
    use solvecraft_geom::Seg2;
    let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, -1.0)).unwrap();
    let segs = vec![
        Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) },
        Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, -20.0) },
        Seg2::Arc { center: Vec2::new(0.0, -20.0), radius: 10.0, start: 0.0, sweep: -PI / 2.0 },
        Seg2::Line { a: Vec2::new(0.0, -30.0), b: Vec2::new(0.0, 0.0) },
    ];
    let b = revolve(&xz, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], Vec2::ZERO, Vec2::new(0.0, -1.0), 2.0 * PI).unwrap().pop().unwrap();
    let s = shell(&b, &[Vec3::new(0.0, 0.0, 0.0)], 1.0).unwrap();
    let outer = PI * 100.0 * 20.0 + 2.0 / 3.0 * PI * 1000.0;
    let inner = PI * 81.0 * 20.0 + 2.0 / 3.0 * PI * 729.0;
    let v = measure(&s).unwrap().volume;
    assert!(rel(v, outer - inner) < 2e-3, "{v} vs {}", outer - inner);
    // A cone frustum (a revolved trapezoid), opened at its wide base.
    let prof =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(5.0, -10.0), Vec2::new(0.0, -10.0)]), holes: vec![] };
    let fr = revolve(&xz, &[prof], Vec2::ZERO, Vec2::new(0.0, -1.0), 2.0 * PI).unwrap().pop().unwrap();
    let v0 = measure(&fr).unwrap().volume;
    let s = shell(&fr, &[Vec3::new(0.0, 0.0, 0.0)], 1.0).unwrap();
    let v = measure(&s).unwrap().volume;
    assert!(v > 0.1 * v0 && v < 0.6 * v0, "{v} of {v0}");
}

#[test]
fn smooth_loft_through_three_sections() {
    // Squares 40 → 20 → 40 wide, 0, 20 and 40 up: the middle is pinched smoothly (no edge there).
    let sq = |s: f64| Loop2::polygon(&[Vec2::new(-s, -s), Vec2::new(s, -s), Vec2::new(s, s), Vec2::new(-s, s)]);
    let l = loft(&[(Plane::XY, sq(20.0)), (Plane::XY.offset(20.0), sq(10.0)), (Plane::XY.offset(40.0), sq(20.0))]).unwrap();
    let m = measure(&l).unwrap();
    // Smooth: between the ruled (two frustums, 2 · 20 · (1600 + 400 + 800)/3) and the box.
    let ruled = 2.0 * 20.0 * (1600.0 + 400.0 + 800.0) / 3.0;
    assert!(m.volume < ruled && m.volume > 40.0 * 400.0, "{}", m.volume);
    // Four sides and two caps; the middle section leaves no edges.
    assert_eq!(m.merged.faces, 6, "{:?}", m.merged);
    assert_eq!(m.merged.edges, 12, "{:?}", m.merged);
    // A circle, a square and a circle: valid and between the inner and outer bounds.
    let c = loft(&[
        (Plane::XY, Loop2::circle(Vec2::ZERO, 10.0)),
        (Plane::XY.offset(15.0), sq(12.0)),
        (Plane::XY.offset(30.0), Loop2::circle(Vec2::ZERO, 8.0)),
    ])
    .unwrap();
    let v = measure(&c).unwrap().volume;
    assert!(v > PI * 64.0 * 30.0 && v < 576.0 * 30.0, "{v}");
}
