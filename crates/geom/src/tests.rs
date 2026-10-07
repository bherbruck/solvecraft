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
    assert_eq!(Plane::XZ.normal(), Vec3::new(0.0, -1.0, 0.0));
    assert_eq!(Plane::YZ.normal(), Vec3::X);
    let p = Plane::XZ.to_world(Vec2::new(3.0, 4.0));
    assert_eq!(p, Vec3::new(3.0, 0.0, 4.0));
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
