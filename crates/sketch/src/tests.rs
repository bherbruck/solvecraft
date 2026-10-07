use std::f64::consts::{FRAC_PI_2, PI};

use solvecraft_geom::Vec2;

use crate::*;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// Rough rectangle from four lines sharing corner points.
fn rect(sk: &mut Sketch, p: [Vec2; 4]) -> [usize; 4] {
    let a = sk.add_point(p[0], None).unwrap();
    let b = sk.add_point(p[1], None).unwrap();
    let c = sk.add_point(p[2], None).unwrap();
    let d = sk.add_point(p[3], None).unwrap();
    [
        sk.add_line_pts(a, b, None).unwrap(),
        sk.add_line_pts(b, c, None).unwrap(),
        sk.add_line_pts(c, d, None).unwrap(),
        sk.add_line_pts(d, a, None).unwrap(),
    ]
}

fn line_pts(sk: &Sketch, l: usize) -> (Vec2, Vec2) {
    match sk.curves[l].kind {
        CurveKind::Line { a, b } => (sk.points[a].pos, sk.points[b].pos),
        _ => panic!(),
    }
}

#[test]
fn fully_constrained_rectangle() {
    let mut sk = Sketch::new();
    let l = rect(&mut sk, [v(0.3, -0.2), v(38.0, 1.0), v(41.0, 29.0), v(-1.0, 31.0)]);
    use ConstraintKind::*;
    sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
    let (a, _) = match sk.curves[l[0]].kind {
        CurveKind::Line { a, b } => (a, b),
        _ => panic!(),
    };
    sk.add_constraint(Coincident { p: a, q: 0 }, None).unwrap();
    sk.add_constraint(Length { l: l[0], value: 40.0 }, Some("d1".into())).unwrap();
    sk.add_constraint(Length { l: l[1], value: 30.0 }, Some("d2".into())).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.dof, 0, "{rep:?}");
    assert!(rep.point_determined.iter().all(|d| *d));
    let (p0, p1) = line_pts(&sk, l[0]);
    assert!(p0.dist(v(0.0, 0.0)) < 1e-7, "{p0:?}");
    assert!(p1.dist(v(40.0, 0.0)) < 1e-7, "{p1:?}");
    let (_, p2) = line_pts(&sk, l[1]);
    assert!(p2.dist(v(40.0, 30.0)) < 1e-7, "{p2:?}");
    // Change a dimension and re-solve.
    sk.constraints[5].kind.set_value(55.0);
    assert!(solve(&mut sk).ok());
    let (_, p1) = line_pts(&sk, l[0]);
    assert!(p1.dist(v(55.0, 0.0)) < 1e-7);
}

#[test]
fn under_constrained_reports_dof() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(10.0, 1.0), None, None, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok());
    assert_eq!(rep.dof, 3);
    let (a, b) = line_pts(&sk, l);
    assert!((a.y - b.y).abs() < 1e-9);
    // Minimal movement: the line moved by about half a unit each.
    assert!((a.y - 0.5).abs() < 0.05 && (b.y - 0.5).abs() < 0.05, "{a:?} {b:?}");
}

#[test]
fn conflicting_constraints_fail_and_restore() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(10.0, 0.0), None, None, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Vertical { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
    let before = sk.clone();
    let rep = solve(&mut sk);
    assert_eq!(rep.status, SolveStatus::Failed);
    assert!(!rep.failing.is_empty());
    assert_eq!(sk.points, before.points);
}

#[test]
fn circles_tangent_equal_concentric() {
    let mut sk = Sketch::new();
    let c1 = sk.add_circle(v(0.0, 0.0), 5.0, Some(0), None).unwrap();
    let c2 = sk.add_circle(v(12.0, 1.0), 3.0, None, None).unwrap();
    use ConstraintKind::*;
    sk.add_constraint(Radius { c: c1, value: 10.0 }, None).unwrap();
    sk.add_constraint(Equal { a: c1, b: c2 }, None).unwrap();
    sk.add_constraint(Tangent { a: c1, b: c2 }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert!((sk.radius(c2).unwrap() - 10.0).abs() < 1e-7);
    assert!((sk.center(c2).unwrap().len() - 20.0).abs() < 1e-7);
    let c3 = sk.add_circle(v(3.0, 3.0), 2.0, None, None).unwrap();
    sk.add_constraint(Concentric { a: c3, b: c2 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    assert!(sk.center(c3).unwrap().dist(sk.center(c2).unwrap()) < 1e-7);
}

#[test]
fn line_tangent_to_circle_and_angles() {
    let mut sk = Sketch::new();
    let c = sk.add_circle(v(0.0, 0.0), 10.0, Some(0), None).unwrap();
    use ConstraintKind::*;
    sk.add_constraint(Radius { c, value: 10.0 }, None).unwrap();
    let l = sk.add_line(v(-20.0, 12.0), v(20.0, 11.0), None, None, None).unwrap();
    sk.add_constraint(Tangent { a: l, b: c }, None).unwrap();
    sk.add_constraint(Horizontal { l }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (a, _) = line_pts(&sk, l);
    assert!((a.y.abs() - 10.0).abs() < 1e-7, "{a:?}");
    let m = sk.add_line(v(0.0, 0.0), v(10.0, 3.0), Some(0), None, None).unwrap();
    sk.add_constraint(Angle { a: l, b: m, value: PI / 6.0, flip: false }, None).unwrap();
    sk.add_constraint(Length { l: m, value: 8.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (p, q) = line_pts(&sk, m);
    let (la, lb) = line_pts(&sk, l);
    assert!(((q - p).angle() - (lb - la).angle() - PI / 6.0).abs() < 1e-7, "{la:?} {lb:?} {p:?} {q:?}");
    let n2 = sk.add_line(v(1.0, 1.0), v(2.0, 5.0), None, None, None).unwrap();
    sk.add_constraint(Perpendicular { a: m, b: n2 }, None).unwrap();
    sk.add_constraint(Parallel { a: n2, b: n2 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (p2, q2) = line_pts(&sk, n2);
    assert!(((q2 - p2).angle() - (PI / 6.0 + FRAC_PI_2)).abs() < 1e-6 || ((q2 - p2).angle() - (PI / 6.0 - FRAC_PI_2)).abs() < 1e-6);
}

#[test]
fn midpoint_symmetry_point_on_curve_arc() {
    let mut sk = Sketch::new();
    use ConstraintKind::*;
    let l = sk.add_line(v(-5.0, 0.0), v(5.0, 0.0), None, None, None).unwrap();
    let p = sk.add_point(v(1.0, 1.0), None).unwrap();
    sk.add_constraint(Midpoint { p, l }, None).unwrap();
    let a = sk.add_arc(v(0.0, 0.0), v(4.0, 0.0), v(0.0, 4.0), [None, None, None], None).unwrap();
    sk.add_constraint(Radius { c: a, value: 6.0 }, None).unwrap();
    let q = sk.add_point(v(3.0, 3.0), None).unwrap();
    sk.add_constraint(PointOnCurve { p: q, c: a }, None).unwrap();
    let s1 = sk.add_point(v(-2.0, 1.0), None).unwrap();
    let s2 = sk.add_point(v(3.0, -1.5), None).unwrap();
    let axis = sk.add_line(v(0.0, -10.0), v(0.0, 10.0), None, None, None).unwrap();
    sk.add_constraint(Vertical { l: axis }, None).unwrap();
    sk.add_constraint(Symmetric { p: s1, q: s2, l: axis }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    let (la, lb) = line_pts(&sk, l);
    assert!(sk.points[p].pos.dist((la + lb) * 0.5) < 1e-7);
    assert!((sk.radius(a).unwrap() - 6.0).abs() < 1e-7);
    let c = sk.center(a).unwrap();
    assert!((sk.points[q].pos.dist(c) - 6.0).abs() < 1e-7);
    let (ax, _) = line_pts(&sk, axis);
    assert!((sk.points[s1].pos.x + sk.points[s2].pos.x - 2.0 * ax.x).abs() < 1e-7);
    assert!((sk.points[s1].pos.y - sk.points[s2].pos.y).abs() < 1e-7);
}

#[test]
fn profiles_rectangle_with_hole_and_split() {
    let mut sk = Sketch::new();
    rect(&mut sk, [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)]);
    sk.add_circle(v(20.0, 15.0), 5.0, None, None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 2, "{ps:?}");
    let big = ps.iter().find(|p| p.region.holes.len() == 1).unwrap();
    assert!((big.area - (1200.0 - PI * 25.0)).abs() < 1e-6);
    let disc = ps.iter().find(|p| p.region.holes.is_empty()).unwrap();
    assert!((disc.area - PI * 25.0).abs() < 1e-6);
    assert_eq!(big.hole_curves.len(), 1);

    // A line across the rectangle splits it into two regions (with the hole in one).
    let mut sk2 = Sketch::new();
    let l = rect(&mut sk2, [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)]);
    let (b0, _) = line_pts(&sk2, l[0]);
    let _ = b0;
    let pa = sk2.add_point(v(10.0, 0.0), None).unwrap();
    let pb = sk2.add_point(v(10.0, 30.0), None).unwrap();
    // Re-split bottom and top edges at x = 10 by rebuilding the sketch as 7 lines.
    let mut s3 = Sketch::new();
    let pts = [v(0.0, 0.0), v(10.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(10.0, 30.0), v(0.0, 30.0)];
    let ids: Vec<usize> = pts.iter().map(|p| s3.add_point(*p, None).unwrap()).collect();
    for i in 0..6 {
        s3.add_line_pts(ids[i], ids[(i + 1) % 6], None).unwrap();
    }
    s3.add_line_pts(ids[1], ids[4], None).unwrap();
    let _ = (pa, pb);
    let ps = find_profiles(&s3);
    assert_eq!(ps.len(), 2);
    let mut areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
    areas.sort_by(f64::total_cmp);
    assert!((areas[0] - 300.0).abs() < 1e-9 && (areas[1] - 900.0).abs() < 1e-9, "{areas:?}");
}

#[test]
fn profile_with_arcs_and_construction_ignored() {
    // Slot: two lines and two half arcs.
    let mut sk = Sketch::new();
    let p = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)];
    let ids: Vec<usize> = p.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    sk.add_line_pts(ids[0], ids[1], None).unwrap();
    sk.add_arc(v(20.0, 5.0), p[1], p[2], [None, Some(ids[1]), Some(ids[2])], None).unwrap();
    sk.add_line_pts(ids[2], ids[3], None).unwrap();
    sk.add_arc(v(0.0, 5.0), p[3], p[0], [None, Some(ids[3]), Some(ids[0])], None).unwrap();
    let c = sk.add_line(v(-5.0, -5.0), v(30.0, 20.0), None, None, None).unwrap();
    sk.curves[c].construction = true;
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert!((ps[0].area - (200.0 + PI * 25.0)).abs() < 1e-6, "{}", ps[0].area);
    assert_eq!(ps[0].outer_curves.len(), 4);
}

#[test]
fn removal_and_references() {
    let mut sk = Sketch::new();
    let l1 = sk.add_line(v(0.0, 0.0), v(1.0, 0.0), None, None, Some("l1")).unwrap();
    let e = sk.resolve_point("l1.end").unwrap();
    let l2 = sk.add_line(v(1.0, 0.0), v(1.0, 1.0), Some(e), None, Some("l2")).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Vertical { l: l2 }, None).unwrap();
    assert!(sk.add_line(v(0.0, 0.0), v(1.0, 0.0), None, None, Some("l1")).is_err());
    sk.remove_curves(&[l1]);
    assert_eq!(sk.curves.len(), 1);
    assert_eq!(sk.constraints.len(), 1);
    assert!(sk.resolve_point("l2.start").is_some());
    assert!(sk.resolve_point("l1.start").is_none());
    assert_eq!(sk.points.len(), 3, "origin + l2's two points");
    assert!(solve(&mut sk).ok());
    let json = serde_json::to_string(&sk).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sk);
}

#[test]
fn hostile_values_rejected() {
    let mut sk = Sketch::new();
    assert!(sk.add_point(v(f64::NAN, 0.0), None).is_err());
    assert!(sk.add_circle(v(0.0, 0.0), -1.0, None, None).is_err());
    assert!(sk.add_circle(v(0.0, 0.0), f64::INFINITY, None, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Horizontal { l: 99 }, None).is_err());
    let c = sk.add_circle(v(0.0, 0.0), 1.0, None, None).unwrap();
    assert!(sk.add_constraint(ConstraintKind::Horizontal { l: c }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Radius { c, value: f64::NAN }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Radius { c, value: -2.0 }, None).is_err());
}

#[test]
fn profiles_split_at_crossings_and_touch_points() {
    // A line across a circle: two half discs.
    let mut sk = Sketch::new();
    sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
    sk.add_line(v(-15.0, 0.0), v(15.0, 0.0), None, None, None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 2, "{ps:?}");
    for p in &ps {
        assert!((p.area - PI * 50.0).abs() < 1e-6, "{}", p.area);
    }
    // Belt: two circles joined by two tangent lines whose ends sit on the circles.
    let mut b = Sketch::new();
    let c1 = b.add_circle(v(0.0, 0.0), 20.0, Some(0), None).unwrap();
    let c2 = b.add_circle(v(50.0, 0.0), 10.0, None, None).unwrap();
    let l1 = b.add_line(v(3.0, 19.0), v(47.0, 13.0), None, None, None).unwrap();
    let l2 = b.add_line(v(1.0, -22.0), v(44.0, -8.0), None, None, None).unwrap();
    use ConstraintKind::*;
    for (l, end, c) in [(l1, "start", c1), (l1, "end", c2), (l2, "start", c1), (l2, "end", c2)] {
        let p = b.resolve_point(&format!("{}.{end}", b.curves[l].id)).unwrap();
        b.add_constraint(PointOnCurve { p, c }, None).unwrap();
    }
    for (l, c) in [(l1, c1), (l1, c2), (l2, c1), (l2, c2)] {
        b.add_constraint(Tangent { a: l, b: c }, None).unwrap();
    }
    b.add_constraint(Radius { c: c1, value: 20.0 }, None).unwrap();
    b.add_constraint(Radius { c: c2, value: 10.0 }, None).unwrap();
    let r = solve(&mut b);
    assert!(r.ok(), "{r:?}");
    let ps = find_profiles(&b);
    assert_eq!(ps.len(), 3, "two discs and the band between them: {:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
    let total: f64 = ps.iter().map(|p| p.area).sum();
    assert!(total > PI * 500.0, "{total}");
}

#[test]
fn merging_adjacent_regions() {
    let a =
        solvecraft_geom::Region2 { outer: solvecraft_geom::Loop2::polygon(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)]), holes: vec![] };
    let b = solvecraft_geom::Region2 {
        outer: solvecraft_geom::Loop2::polygon(&[v(10.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(10.0, 10.0)]),
        holes: vec![],
    };
    let c = solvecraft_geom::Region2 { outer: solvecraft_geom::Loop2::polygon(&[v(50.0, 0.0), v(60.0, 0.0), v(60.0, 10.0)]), holes: vec![] };
    let m = merge_regions(&[a, b, c]);
    assert_eq!(m.len(), 2);
    let mut areas: Vec<f64> = m.iter().map(|r| r.area()).collect();
    areas.sort_by(f64::total_cmp);
    assert!((areas[0] - 50.0).abs() < 1e-9 && (areas[1] - 200.0).abs() < 1e-9, "{areas:?}");
}

#[test]
fn links_update_in_place_rebuild_and_break() {
    use crate::{LinkGeom, LinkKind, LinkSource};
    let mut sk = Sketch::new();
    let src = LinkSource::Origin;
    let sq = |s: f64| {
        vec![
            LinkGeom::Line(v(0.0, 0.0), v(s, 0.0)),
            LinkGeom::Line(v(s, 0.0), v(s, s)),
            LinkGeom::Line(v(s, s), v(0.0, s)),
            LinkGeom::Line(v(0.0, s), v(0.0, 0.0)),
        ]
    };
    let j = sk.add_link(LinkKind::Project, src, &sq(10.0)).unwrap();
    assert_eq!(sk.link_curves(&j).len(), 4);
    // Corners are shared: 4 points, all fixed.
    let pts = sk.link_points(&j);
    assert_eq!(pts.len(), 4);
    assert!(pts.iter().all(|p| sk.points[*p].fixed));
    // A free line dimensioned to a projected corner keeps its constraint across an update.
    let l = sk.add_line(v(3.0, 3.0), v(5.0, 7.0), None, None, None).unwrap();
    let (la, _) = match sk.curves[l].kind {
        CurveKind::Line { a, b } => (a, b),
        _ => panic!(),
    };
    sk.add_constraint(ConstraintKind::Coincident { p: la, q: pts[2] }, None).unwrap();
    let ids: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert!(!sk.update_link(&j, &sq(20.0)).unwrap(), "same shape updates in place");
    let ids2: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids, ids2);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[la].pos.dist(v(20.0, 20.0)) < 1e-9);
    assert_eq!(find_profiles(&sk).len(), 1);
    // A different shape rebuilds (the coincidence on the old corner goes).
    assert!(sk.update_link(&j, &[LinkGeom::Circle(v(1.0, 1.0), 4.0)]).unwrap());
    assert_eq!(sk.link_curves(&j).len(), 1);
    assert_eq!(sk.constraints.len(), 0);
    // Linked circles keep their radius in the solver.
    let c = sk.link_curves(&j)[0];
    let rep = solve(&mut sk);
    assert!(rep.curve_determined[c], "{rep:?}");
    // Breaking the link frees it.
    sk.break_link(&j).unwrap();
    assert!(sk.links.is_empty());
    assert!(sk.curves[c].link.is_none());
    let rep = solve(&mut sk);
    assert!(!rep.curve_determined[c]);
    // Deleting all of a link's curves removes the link.
    let j2 = sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(v(0.0, 0.0), v(1.0, 0.0))]).unwrap();
    let cs = sk.link_curves(&j2);
    sk.remove_curves(&cs);
    assert!(sk.link(&j2).is_none());
    // Nothing to project is an error, not an empty link.
    assert!(sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(v(0.0, 0.0), v(0.0, 0.0))]).is_err());
    // Round trip through JSON.
    let j3 = sk.add_link(LinkKind::Intersect, LinkSource::Axis { name: "X".into() }, &[LinkGeom::Point(v(2.0, 2.0))]).unwrap();
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&sk).unwrap()).unwrap();
    assert_eq!(back, sk);
    assert_eq!(back.link(&j3).unwrap().kind, LinkKind::Intersect);
}
