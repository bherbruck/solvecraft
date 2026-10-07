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
    // Adjacent edges at a blended corner are not supported yet (an error, not a crash).
    assert!(fillet(&b, &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 15.0, 20.0)], 2.0).is_err());
    // Concave edge: not supported.
    let u = boolean(&b, &box_solid(Vec3::new(10.0, 10.0, 15.0), Vec3::new(20.0, 20.0, 30.0)).unwrap(), BoolOp::Union).unwrap().unwrap();
    assert!(fillet(&u, &[Vec3::new(15.0, 10.0, 20.0)], 1.0).is_err());
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
