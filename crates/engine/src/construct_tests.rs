//! Construction axes, points and planes (SOLID ▸ CONSTRUCT), one test per type, against a
//! 40 × 30 × 20 box at the origin, a cylinder, a sphere and a torus.

use serde_json::{Value, json};
use solvecraft_geom::{Plane, Vec3};

use crate::Session;
use crate::view::{construction_axes, construction_planes, construction_points};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn part() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Box"}));
    run(&mut s, "solid.cylinder", json!({"base": [60, 0, 0], "radius": 5, "height": 10, "body_name": "Cyl"}));
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 60], "radius": 5, "body_name": "Ball"}));
    run(&mut s, "solid.torus", json!({"center": [0, 60, 0], "major": 10, "minor": 2, "body_name": "Ring"}));
    s
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-3
}

fn parallel(a: Vec3, b: Vec3) -> bool {
    a.normalized().zip(b.normalized()).is_some_and(|(a, b)| a.cross(b).len() < 1e-6)
}

fn axis(s: &Session, name: &str) -> (Vec3, Vec3) {
    construction_axes(s).into_iter().find(|a| a.1 == name).map(|a| (a.2, a.3)).unwrap_or_else(|| panic!("no axis {name}"))
}

fn point(s: &Session, name: &str) -> Vec3 {
    construction_points(s).into_iter().find(|p| p.1 == name).map(|p| p.2).unwrap_or_else(|| panic!("no point {name}"))
}

fn plane(s: &Session, name: &str) -> Plane {
    construction_planes(s).into_iter().find(|p| p.1 == name).map(|p| p.2).unwrap_or_else(|| panic!("no plane {name}"))
}

/// The line through `o` along `d` contains `p`.
fn on_line(o: Vec3, d: Vec3, p: Vec3) -> bool {
    let d = d.normalized().unwrap_or(Vec3::Z);
    (p - o - d * (p - o).dot(d)).len() < 1e-3
}

fn on_plane(pl: &Plane, p: Vec3) -> bool {
    (p - pl.origin).dot(pl.normal()).abs() < 1e-3
}

#[test]
fn axis_through_cylinder_cone_or_torus() {
    let mut s = part();
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [65, 0, 5]}, "name": "A"}));
    let (o, d) = axis(&s, "A");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(60.0, 0.0, 0.0)), "{o:?} {d:?}");
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [12, 60, 0]}, "name": "T"}));
    let (o, d) = axis(&s, "T");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(0.0, 60.0, 0.0)), "{o:?} {d:?}");
    // A flat face has no axis.
    assert!(s.execute("construct.axis.through_cylinder", &json!({"face": {"face": [20, 15, 20]}})).is_err());
}

#[test]
fn axis_perpendicular_to_a_face_at_a_point() {
    let mut s = part();
    run(&mut s, "construct.axis.normal_to_face", json!({"face": {"face": [10, 10, 20]}, "point": {"point": [10, 10, 25]}, "name": "N"}));
    let (o, d) = axis(&s, "N");
    assert!(near(o, Vec3::new(10.0, 10.0, 20.0)) && parallel(d, Vec3::Z), "{o:?} {d:?}");
    // On the cylinder: radial through the point.
    run(&mut s, "construct.axis.normal_to_face", json!({"face": {"face": [65, 0, 5]}, "point": {"point": [70, 0, 5]}, "name": "R"}));
    let (o, d) = axis(&s, "R");
    assert!(near(o, Vec3::new(65.0, 0.0, 5.0)) && parallel(d, Vec3::X), "{o:?} {d:?}");
}

#[test]
fn axis_through_two_planes() {
    let mut s = part();
    run(&mut s, "construct.axis.two_planes", json!({"a": {"plane": "XZ"}, "b": {"face": [40, 15, 10]}, "name": "L"}));
    let (o, d) = axis(&s, "L");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(40.0, 0.0, 0.0)), "{o:?} {d:?}");
    assert!(s.execute("construct.axis.two_planes", &json!({"a": "XY", "b": {"face": [20, 15, 20]}})).is_err(), "parallel");
}

#[test]
fn axis_through_two_points() {
    let mut s = part();
    run(&mut s, "construct.axis.two_points", json!({"a": {"vertex": [0.5, 0.5, 0.5]}, "b": {"vertex": [39, 29, 19]}, "name": "D"}));
    let (o, d) = axis(&s, "D");
    assert!(on_line(o, d, Vec3::ZERO) && parallel(d, Vec3::new(40.0, 30.0, 20.0)), "{o:?} {d:?}");
}

#[test]
fn axis_through_an_edge() {
    let mut s = part();
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [20, 0, 0]}, "name": "E"}));
    let (o, d) = axis(&s, "E");
    assert!(parallel(d, Vec3::X) && on_line(o, d, Vec3::ZERO), "{o:?} {d:?}");
    // A circular edge is not straight.
    assert!(s.execute("construct.axis.edge", &json!({"edge": {"edge": [65, 0, 10]}})).is_err());
}

#[test]
fn point_at_a_vertex() {
    let mut s = part();
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [39, 29, 19]}, "name": "P"}));
    assert!(near(point(&s, "P"), Vec3::new(40.0, 30.0, 20.0)));
}

#[test]
fn point_through_two_edges() {
    let mut s = part();
    run(&mut s, "construct.point.two_edges", json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [40, 15, 0]}, "name": "P"}));
    assert!(near(point(&s, "P"), Vec3::new(40.0, 0.0, 0.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_through_three_planes() {
    let mut s = part();
    run(
        &mut s,
        "construct.point.three_planes",
        json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "c": {"face": [20, 15, 20]}, "name": "P"}),
    );
    assert!(near(point(&s, "P"), Vec3::new(40.0, 30.0, 20.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_at_the_centre_of_a_circle_sphere_or_torus() {
    let mut s = part();
    run(&mut s, "construct.point.center", json!({"of": {"edge": [65, 0, 10]}, "name": "C"}));
    assert!(near(point(&s, "C"), Vec3::new(60.0, 0.0, 10.0)), "{:?}", point(&s, "C"));
    run(&mut s, "construct.point.center", json!({"of": {"face": [5, 0, 60]}, "name": "S"}));
    assert!(near(point(&s, "S"), Vec3::new(0.0, 0.0, 60.0)), "{:?}", point(&s, "S"));
    run(&mut s, "construct.point.center", json!({"of": {"face": [12, 60, 0]}, "name": "T"}));
    assert!(near(point(&s, "T"), Vec3::new(0.0, 60.0, 0.0)), "{:?}", point(&s, "T"));
}

#[test]
fn point_at_edge_and_plane() {
    let mut s = part();
    run(
        &mut s,
        "construct.point.edge_plane",
        json!({"edge": {"edge": [20, 0, 0]}, "plane": {"plane": {"origin": [12, 0, 0], "normal": [1, 0, 0]}}, "name": "P"}),
    );
    assert!(near(point(&s, "P"), Vec3::new(12.0, 0.0, 0.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_along_a_path() {
    let mut s = part();
    run(&mut s, "construct.point.along_path", json!({"path": {"edge": [20, 0, 0]}, "t": 0.25, "name": "P"}));
    let p = point(&s, "P");
    assert!(near(p, Vec3::new(10.0, 0.0, 0.0)) || near(p, Vec3::new(30.0, 0.0, 0.0)), "{p:?}");
}

#[test]
fn midplane_between_parallel_and_meeting_faces() {
    let mut s = part();
    run(&mut s, "construct.plane.midplane", json!({"a": "XY", "b": {"face": [20, 15, 20]}, "name": "M"}));
    let m = plane(&s, "M");
    assert!(parallel(m.normal(), Vec3::Z) && on_plane(&m, Vec3::new(5.0, 5.0, 10.0)), "{m:?}");
    // Two sides meeting at the corner (40, 30): the diagonal plane through it.
    run(&mut s, "construct.plane.midplane", json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "name": "Q"}));
    let q = plane(&s, "Q");
    assert!(on_plane(&q, Vec3::new(40.0, 30.0, 0.0)) && on_plane(&q, Vec3::new(41.0, 31.0, 5.0)), "{q:?}");
}

#[test]
fn plane_through_two_edges() {
    let mut s = part();
    run(&mut s, "construct.plane.two_edges", json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [20, 30, 20]}, "name": "E"}));
    let e = plane(&s, "E");
    // The front-bottom and back-top edges: the slanted plane through both.
    assert!(on_plane(&e, Vec3::new(5.0, 0.0, 0.0)) && on_plane(&e, Vec3::new(5.0, 30.0, 20.0)), "{e:?}");
    assert!(s.execute("construct.plane.two_edges", &json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [40, 15, 20]}})).is_err(), "skew");
}

#[test]
fn plane_through_three_points() {
    let mut s = part();
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"vertex": [0, 0, 20]}, "b": {"vertex": [40, 0, 20]}, "c": {"vertex": [40, 30, 0]}, "name": "T"}),
    );
    let t = plane(&s, "T");
    assert!(on_plane(&t, Vec3::new(0.0, 0.0, 20.0)) && on_plane(&t, Vec3::new(40.0, 30.0, 0.0)) && on_plane(&t, Vec3::new(0.0, 30.0, 0.0)), "{t:?}");
}

#[test]
fn plane_tangent_to_a_cylinder() {
    let mut s = part();
    run(&mut s, "construct.plane.tangent", json!({"face": {"face": [65, 0, 5]}, "name": "T"}));
    let t = plane(&s, "T");
    assert!(parallel(t.normal(), Vec3::X) && on_plane(&t, Vec3::new(65.0, 3.0, 9.0)), "{t:?}");
    // And to the sphere, where the pick lies over it.
    run(&mut s, "construct.plane.tangent", json!({"face": {"face": [0, 0, 65]}, "name": "U"}));
    let u = plane(&s, "U");
    assert!(parallel(u.normal(), Vec3::Z) && on_plane(&u, Vec3::new(3.0, 3.0, 65.0)), "{u:?}");
}

#[test]
fn plane_along_a_path() {
    let mut s = part();
    run(&mut s, "construct.plane.along_path", json!({"path": {"edge": [20, 0, 0]}, "t": 0.5, "name": "P"}));
    let p = plane(&s, "P");
    assert!(parallel(p.normal(), Vec3::X) && on_plane(&p, Vec3::new(20.0, 7.0, 3.0)), "{p:?}");
    // A sketch can be placed on it.
    run(&mut s, "sketch.create", json!({"plane": "P", "name": "OnPath"}));
}

/// Built from model geometry, they follow when the model changes: the box gets taller.
#[test]
fn construction_follows_the_model() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "h", "body_name": "Box"}));
    run(
        &mut s,
        "construct.point.three_planes",
        json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "c": {"face": [20, 15, 20]}, "name": "Corner"}),
    );
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [20, 30, 20]}, "name": "TopBack"}));
    run(&mut s, "construct.plane.midplane", json!({"a": "XY", "b": {"face": [20, 15, 20]}, "name": "Mid"}));
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "50 mm"}));
    assert!(near(point(&s, "Corner"), Vec3::new(40.0, 30.0, 50.0)), "{:?}", point(&s, "Corner"));
    let (o, d) = axis(&s, "TopBack");
    assert!(parallel(d, Vec3::X) && on_line(o, d, Vec3::new(0.0, 30.0, 50.0)), "{o:?} {d:?}");
    assert!(on_plane(&plane(&s, "Mid"), Vec3::new(1.0, 1.0, 25.0)));
    // Construction from construction: an axis through two construction points.
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [0, 0, 0]}, "name": "Origin"}));
    run(&mut s, "construct.axis.two_points", json!({"a": {"construct_point": "Origin"}, "b": {"construct_point": "Corner"}, "name": "Diag"}));
    let (o, d) = axis(&s, "Diag");
    assert!(on_line(o, d, Vec3::new(40.0, 30.0, 50.0)) && on_line(o, d, Vec3::ZERO));
}

/// In a component moved 50 mm along X, picks given in the world land on its geometry, and the
/// axis and point move with the occurrence.
#[test]
fn construction_lives_in_the_component_frame() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "body_name": "Pin"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [55, 0, 5]}, "name": "PinAxis"}));
    run(&mut s, "construct.point.center", json!({"of": {"edge": [55, 0, 10]}, "name": "PinTop"}));
    let (o, d) = axis(&s, "PinAxis");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(50.0, 0.0, 0.0)), "{o:?} {d:?}");
    assert!(near(point(&s, "PinTop"), Vec3::new(50.0, 0.0, 10.0)), "{:?}", point(&s, "PinTop"));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 20, 0]}));
    assert!(near(point(&s, "PinTop"), Vec3::new(50.0, 20.0, 10.0)), "{:?}", point(&s, "PinTop"));
}
