use crate::*;

fn cube_mesh(s: f64) -> Mesh {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x * s, y * s, z * s);
    let positions = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)];
    let triangles =
        vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
    let n = triangles.len();
    Mesh { normals: vec![Vec3::Z; 8], positions, triangles, tri_face: vec![0; n], edges: vec![], seams: vec![], edge_faces: vec![] }
}

#[test]
fn cube_measure() {
    let m = cube_mesh(2.0).measure();
    assert!((m.volume - 8.0).abs() < 1e-12);
    assert!((m.area - 24.0).abs() < 1e-12);
    assert!(m.centroid.dist(Vec3::new(1.0, 1.0, 1.0)) < 1e-12);
}

#[test]
fn raycast_hits_front_face() {
    let m = cube_mesh(2.0);
    let (t, _) = m.raycast(Vec3::new(1.0, 1.0, 10.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
    assert!((t - 8.0).abs() < 1e-9);
    assert!(m.raycast(Vec3::new(5.0, 5.0, 10.0), Vec3::new(0.0, 0.0, -1.0)).is_none());
}

#[test]
fn planes() {
    assert_eq!(Plane::XY.normal(), Vec3::Z);
    assert_eq!(Plane::XZ.normal(), Vec3::Y);
    assert_eq!(Plane::YZ.normal(), Vec3::X);
    let p = Plane::XZ.to_world(Vec2::new(3.0, 4.0));
    assert_eq!(p, Vec3::new(3.0, 0.0, -4.0));
    assert_eq!(Plane::XZ.to_local(p), Vec2::new(3.0, 4.0));
    let hit = Plane::XY.offset(5.0).intersect_ray(Vec3::new(1.0, 2.0, 10.0), Vec3::new(0.0, 0.0, -2.0)).unwrap();
    assert!(hit.dist(Vec3::new(1.0, 2.0, 5.0)) < 1e-12);
    assert!(Plane::new(Vec3::ZERO, Vec3::X, Vec3::X).is_none());
    let f = Plane::from_normal(Vec3::ZERO, Vec3::new(0.0, 0.0, 3.0)).unwrap();
    assert!((f.normal() - Vec3::Z).len() < 1e-12);
    assert!(Plane::from_normal(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0)).is_none());
}

#[test]
fn vec_ops_and_serde() {
    let a = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(a.cross(Vec3::X), Vec3::new(0.0, 3.0, -2.0));
    assert_eq!(Vec2::new(3.0, 4.0).len(), 5.0);
    assert!(Vec2::ZERO.normalized().is_none());
    assert!(Vec3::new(0.5, 0.0, 0.0).dist_to_segment(Vec3::ZERO, Vec3::X) < 1e-12);
    assert_eq!(serde_json::to_string(&a).unwrap(), "[1.0,2.0,3.0]");
    let b: Vec2 = serde_json::from_str("[1,2]").unwrap();
    assert_eq!(b, Vec2::new(1.0, 2.0));
}

#[test]
fn aabb() {
    let mut b = Aabb3::EMPTY;
    assert!(b.is_empty());
    b.add(Vec3::new(1.0, 2.0, 3.0));
    b.add(Vec3::new(f64::NAN, 0.0, 0.0));
    b.add(Vec3::new(-1.0, 0.0, 0.0));
    assert_eq!(b.size(), Vec3::new(2.0, 2.0, 3.0));
}

#[test]
fn tangent_chains_follow_smooth_edges() {
    // A line, then an arc leaving it tangentially, then a sharp corner.
    let mut m = Mesh { positions: vec![Vec3::ZERO, Vec3::new(30.0, 30.0, 0.0)], ..Default::default() };
    m.edges.push(vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)]);
    let arc: Vec<Vec3> = (0..=16)
        .map(|k| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * k as f64 / 16.0;
            Vec3::new(10.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin(), 0.0)
        })
        .collect();
    m.edges.push(arc);
    m.edges.push(vec![Vec3::new(15.0, 5.0, 0.0), Vec3::new(5.0, 5.0, 0.0)]);
    m.edge_faces = vec![vec![0], vec![0, 1], vec![1]];
    let mut c = m.tangent_chain(0, 5f64.to_radians());
    c.sort();
    assert_eq!(c, vec![0, 1]);
    assert_eq!(m.tangent_chain(2, 5f64.to_radians()), vec![2]);
    assert_eq!(m.face_edges(1), vec![1, 2]);
}

#[test]
fn cubic_and_conic_segments_measure_exactly() {
    use crate::{Loop2, Seg2, Vec2};
    // An ellipse (a = 5, b = 3) as four quarter conics: area π a b.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let (a, b) = (5.0, 3.0);
    let q = |p0: Vec2, c: Vec2, p1: Vec2| Seg2::Conic { a: p0, apex: c, b: p1, w };
    let e = Loop2 {
        segs: vec![
            q(Vec2::new(a, 0.0), Vec2::new(a, b), Vec2::new(0.0, b)),
            q(Vec2::new(0.0, b), Vec2::new(-a, b), Vec2::new(-a, 0.0)),
            q(Vec2::new(-a, 0.0), Vec2::new(-a, -b), Vec2::new(0.0, -b)),
            q(Vec2::new(0.0, -b), Vec2::new(a, -b), Vec2::new(a, 0.0)),
        ],
    };
    assert!((e.signed_area() - std::f64::consts::PI * a * b).abs() < 1e-9, "{}", e.signed_area());
    for s in &e.segs {
        let m = s.point_at(0.37);
        assert!(((m.x / a).powi(2) + (m.y / b).powi(2) - 1.0).abs() < 1e-12);
    }
    // A cubic on a straight line has the line's length; reversing flips the area sign.
    let c = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 0.0), p2: Vec2::new(2.0, 0.0), p3: Vec2::new(3.0, 0.0) };
    assert!((c.length() - 3.0).abs() < 1e-12);
    // Square with one bulging cubic side: area = 1 + the cubic's area over the chord.
    let bulge = Seg2::Cubic { p0: Vec2::new(1.0, 0.0), p1: Vec2::new(1.5, 1.0 / 3.0), p2: Vec2::new(1.5, 2.0 / 3.0), p3: Vec2::new(1.0, 1.0) };
    let sq = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(1.0, 0.0) },
            bulge,
            Seg2::Line { a: Vec2::new(1.0, 1.0), b: Vec2::new(0.0, 1.0) },
            Seg2::Line { a: Vec2::new(0.0, 1.0), b: Vec2::new(0.0, 0.0) },
        ],
    };
    // x(t) - 1 = 1.5 t(1-t) (Bernstein: 3·0.5·t(1-t)²+3·0.5·t²(1-t)), y = t: ∫ 1.5 t(1-t) dt = 0.25.
    assert!((sq.signed_area() - 1.25).abs() < 1e-12, "{}", sq.signed_area());
    let rev = bulge.reversed();
    assert!((rev.area_term() + bulge.area_term()).abs() < 1e-12);
    assert!(bulge.polyline(1e-4).len() > 4);
    // Halves cover the same curve.
    let (l, r) = bulge.split_half();
    assert!(l.point_at(0.5).dist(bulge.point_at(0.25)) < 1e-12);
    assert!(r.point_at(0.5).dist(bulge.point_at(0.75)) < 1e-12);
    // Conic halves are re-parameterised but stay on the ellipse.
    let (l2, r2) = e.segs[0].split_half();
    for t in [0.1, 0.5, 0.9] {
        for q in [l2.point_at(t), r2.point_at(t)] {
            assert!(((q.x / a).powi(2) + (q.y / b).powi(2) - 1.0).abs() < 1e-12, "{q:?}");
        }
    }
    for (seg, (l, r)) in [(bulge, (l, r)), (e.segs[0], (l2, r2))] {
        assert!(l.end().dist(r.start()) < 1e-12);
        assert!((l.area_term() + r.area_term() - seg.area_term()).abs() < 1e-12);
    }
}

#[test]
fn split_at_and_closest_param() {
    use crate::{Seg2, Vec2};
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = Seg2::Conic { a: Vec2::new(10.0, 0.0), apex: Vec2::new(10.0, 10.0), b: Vec2::new(0.0, 10.0), w };
    for t in [0.2, 0.5, 0.8] {
        let (l, r) = quarter.split_at(t);
        assert!(l.end().dist(quarter.point_at(t)) < 1e-12 && r.start().dist(l.end()) < 1e-12);
        for s in [0.3, 0.7] {
            assert!((l.point_at(s).len() - 10.0).abs() < 1e-12 && (r.point_at(s).len() - 10.0).abs() < 1e-12);
        }
        assert!((l.area_term() + r.area_term() - quarter.area_term()).abs() < 1e-12);
    }
    let c = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 2.0), p2: Vec2::new(3.0, 2.0), p3: Vec2::new(4.0, 0.0) };
    let q = c.point_at(0.37);
    assert!((c.closest_param(q + Vec2::new(0.0, 0.0)) - 0.37).abs() < 1e-9);
    let (l, r) = c.split_at(0.37);
    assert!(l.end().dist(q) < 1e-12 && r.start().dist(q) < 1e-12);
    assert!((quarter.closest_param(Vec2::new(20.0, 20.0)) - 0.5).abs() < 1e-9);
}
