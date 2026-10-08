//! STEP (ISO 10303-21) export: AP242 product structure, assemblies, names and colours.
//!
//! Each body's B-rep is written by truck's STEP writer (geometry and topology only), read back
//! with our Part 21 parser and copied into a file we assemble ourselves: one header and
//! context, a product per part with its shape representation, named solids, surface colours,
//! and assemblies as next-assembly-usage occurrences placed by item-defined transformations
//! (the structure the CAx-IF recommends and our reader follows). Identical geometric entities
//! (points, directions, placements, curves, surfaces) are written once and referenced; ids are
//! dense from `#1`.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::body::Body;
use crate::step_in::p21::{self, Param};
use crate::{KernelError, Result};

/// A named, optionally coloured body to export (coordinates in its part's frame, mm).
pub struct ExportBody<'a> {
    pub name: String,
    pub body: &'a Body,
    pub color: Option<[f32; 3]>,
}

/// A part or assembly: its own bodies and placed child products.
pub struct ExportProduct<'a> {
    pub name: String,
    pub bodies: Vec<ExportBody<'a>>,
    /// (child product index, rigid placement in this product, column-major 4×4, occurrence name).
    pub children: Vec<(usize, [[f64; 4]; 4], String)>,
}

/// Header fields.
#[derive(Clone, Debug, Default)]
pub struct StepHeader {
    /// FILE_NAME name (the file's own name).
    pub file_name: String,
    /// FILE_NAME time stamp (ISO 8601), empty when unknown.
    pub time_stamp: String,
    pub author: String,
    pub organization: String,
}

/// Entities whose identity is topological: never merged even when their text is equal.
const TOPOLOGY: &[&str] = &[
    "VERTEX_POINT",
    "EDGE_CURVE",
    "ORIENTED_EDGE",
    "EDGE_LOOP",
    "FACE_BOUND",
    "FACE_OUTER_BOUND",
    "FACE_SURFACE",
    "ADVANCED_FACE",
    "ORIENTED_FACE",
    "CLOSED_SHELL",
    "OPEN_SHELL",
    "ORIENTED_CLOSED_SHELL",
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
];

fn real(x: f64) -> String {
    if !x.is_finite() {
        return "0.".into();
    }
    let s = format!("{x:?}");
    match s.split_once('e') {
        Some((m, e)) => {
            let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
            format!("{m}E{e}")
        }
        None if s.contains('.') => s,
        None => format!("{s}."),
    }
}

/// A STEP string literal (quotes doubled, non-ASCII as `\X2\` UTF-16 runs).
fn string(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => o.push_str("''"),
            '\\' => o.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => o.push(c),
            c if c.is_ascii_control() => {}
            c => {
                o.push_str("\\X2\\");
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(o, "{u:04X}");
                }
                o.push_str("\\X0\\");
            }
        }
    }
    o.push('\'');
    o
}

fn param(p: &Param, map: &HashMap<u64, u64>, o: &mut String) -> std::result::Result<(), String> {
    match p {
        Param::Int(i) => {
            let _ = write!(o, "{i}");
        }
        Param::Real(x) => o.push_str(&real(*x)),
        Param::Str(s) => o.push_str(&string(s)),
        Param::Enum(e) => {
            let _ = write!(o, ".{e}.");
        }
        Param::Ref(r) => {
            let n = map.get(r).ok_or_else(|| format!("dangling reference #{r}"))?;
            let _ = write!(o, "#{n}");
        }
        Param::List(v) => {
            o.push('(');
            params(v, map, o)?;
            o.push(')');
        }
        Param::Typed(name, v) => {
            o.push_str(name);
            o.push('(');
            params(v, map, o)?;
            o.push(')');
        }
        Param::Binary(b) => {
            let _ = write!(o, "\"{b}\"");
        }
        Param::Null => o.push('$'),
        Param::Derived => o.push('*'),
    }
    Ok(())
}

fn params(v: &[Param], map: &HashMap<u64, u64>, o: &mut String) -> std::result::Result<(), String> {
    for (i, p) in v.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        param(p, map, o)?;
    }
    Ok(())
}

/// The DATA section under construction.
#[derive(Default)]
struct Out {
    lines: Vec<String>,
    /// Geometry text → id (shared entities).
    shared: HashMap<String, u64>,
    /// Surfaces written with the opposite normal to the one they replace.
    flipped: std::collections::HashSet<u64>,
    /// B-spline cylinder sheets of the body being copied under merged faces, with the direction
    /// of the cylinder's parameter seam.
    merged: HashMap<u64, V3>,
    /// Faces of the body being copied: exchange id → written id.
    faces_out: HashMap<u64, u64>,
}

impl Out {
    fn add(&mut self, text: impl Into<String>) -> u64 {
        self.lines.push(text.into());
        self.lines.len() as u64
    }
    /// Add, or reuse an identical earlier entity (geometry only).
    fn add_shared(&mut self, text: String) -> u64 {
        if let Some(id) = self.shared.get(&text) {
            return *id;
        }
        let id = self.add(text.clone());
        self.shared.insert(text, id);
        id
    }
    fn point(&mut self, p: [f64; 3]) -> u64 {
        self.add_shared(format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }
    fn direction(&mut self, d: [f64; 3]) -> u64 {
        self.add_shared(format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2])))
    }
    fn write_analytic(&mut self, a: &Analytic) -> u64 {
        match a.kind {
            AnalyticKind::Plane { o, z, x } => {
                let ax = self.placement(o, z, x);
                self.add_shared(format!("PLANE('',#{ax})"))
            }
            AnalyticKind::Cylinder { o, z, x, r } => {
                let ax = self.placement(o, z, x);
                self.add_shared(format!("CYLINDRICAL_SURFACE('',#{ax},{})", real(r)))
            }
            AnalyticKind::Cone { o, z, x, r, angle } => {
                let ax = self.placement(o, z, x);
                self.add_shared(format!("CONICAL_SURFACE('',#{ax},{},{})", real(r), real(angle)))
            }
            AnalyticKind::Sphere { o, z, x, r } => {
                let ax = self.placement(o, z, x);
                self.add_shared(format!("SPHERICAL_SURFACE('',#{ax},{})", real(r)))
            }
            AnalyticKind::Torus { o, z, x, big, small } => {
                let ax = self.placement(o, z, x);
                self.add_shared(format!("TOROIDAL_SURFACE('',#{ax},{},{})", real(big), real(small)))
            }
            AnalyticKind::HornTorus { o, z, x, big, small } => {
                // Nine-point rational quadratic circles (quarter arcs, corner weights √½): the
                // tube from the pole round and back, and the turn about z from x.
                let y = cross(z, x);
                let ring = |k: usize| {
                    let ang = std::f64::consts::FRAC_PI_4 * k as f64;
                    let (scale, w) = if k % 2 == 1 { (std::f64::consts::SQRT_2, std::f64::consts::FRAC_1_SQRT_2) } else { (1.0, 1.0) };
                    (ang.cos() * scale, ang.sin() * scale, w)
                };
                let (mut rows, mut weights) = (Vec::new(), Vec::new());
                for i in 0..9 {
                    let (ci, si, wi) = ring(i);
                    let (rho, h) = (big - small * ci, small * si);
                    let (mut row, mut wrow) = (Vec::new(), Vec::new());
                    for j in 0..9 {
                        let (cj, sj, wj) = ring(j);
                        let pt = add(add(o, mul(z, h)), add(mul(x, rho * cj), mul(y, rho * sj)));
                        row.push(format!("#{}", self.point(pt)));
                        wrow.push(real(wi * wj));
                    }
                    rows.push(format!("({})", row.join(",")));
                    weights.push(format!("({})", wrow.join(",")));
                }
                self.add_shared(format!(
                    "(BOUNDED_SURFACE()B_SPLINE_SURFACE(2,2,({}),.UNSPECIFIED.,.T.,.T.,.F.)B_SPLINE_SURFACE_WITH_KNOTS((3,2,2,2,3),(3,2,2,2,3),(0.,0.25,0.5,0.75,1.),(0.,0.25,0.5,0.75,1.),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_SURFACE(({}))REPRESENTATION_ITEM('')SURFACE())",
                    rows.join(","),
                    weights.join(",")
                ))
            }
        }
    }
    fn placement(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u64 {
        let (po, dz, dx) = (self.point(o), self.direction(z), self.direction(x));
        self.add_shared(format!("AXIS2_PLACEMENT_3D('',#{po},#{dz},#{dx})"))
    }

    /// Copy a solid entity of a parsed file and everything it uses; returns its new id.
    fn copy_solid(&mut self, ex: &p21::Exchange, root: u64, name: &str) -> std::result::Result<u64, String> {
        let mut map: HashMap<u64, u64> = HashMap::new();
        let seams = horn_seams(ex);
        // Iterative post-order walk (children before parents).
        let mut stack: Vec<(u64, bool)> = vec![(root, false)];
        let mut guard = 0usize;
        while let Some((id, done)) = stack.pop() {
            guard += 1;
            if guard > 50_000_000 {
                return Err("solid too large".into());
            }
            if map.contains_key(&id) {
                continue;
            }
            let e = ex.get(id).ok_or_else(|| format!("missing #{id}"))?;
            // Revolutions with analytic profiles become cylinders, cones, spheres, tori and
            // planes (what other systems recognise), with the face sense corrected.
            if !done
                && e.name() == "SURFACE_OF_REVOLUTION"
                && let Some(mut a) = analytic_revolution(ex, e)
            {
                if let AnalyticKind::HornTorus { x, .. } = &mut a.kind
                    && let Some(s) = seams.get(&id)
                {
                    *x = *s;
                }
                let new = self.write_analytic(&a);
                if a.flip {
                    self.flipped.insert(new);
                }
                map.insert(id, new);
                continue;
            }
            // Flat B-spline sheets are planes; cylindrical ones become cylinders when their
            // pieces were merged into one face (a lone piece stays as written).
            if !done
                && let Some(a) = planar_bspline(ex, e).map(|kind| Analytic { kind, flip: false }).or_else(|| {
                    let seam = self.merged.get(&id)?;
                    let mut a = cylindrical_bspline(ex, e)?;
                    if let AnalyticKind::Cylinder { x, .. } = &mut a.kind {
                        *x = *seam;
                    }
                    Some(a)
                })
            {
                let new = self.write_analytic(&a);
                if a.flip {
                    self.flipped.insert(new);
                }
                map.insert(id, new);
                continue;
            }
            if !done {
                stack.push((id, true));
                let mut refs = Vec::new();
                fn collect(p: &Param, out: &mut Vec<u64>) {
                    match p {
                        Param::Ref(r) => out.push(*r),
                        Param::List(v) | Param::Typed(_, v) => v.iter().for_each(|x| collect(x, out)),
                        _ => {}
                    }
                }
                e.records.iter().flat_map(|r| &r.params).for_each(|p| collect(p, &mut refs));
                stack.extend(refs.into_iter().filter(|r| !map.contains_key(r)).map(|r| (r, false)));
                continue;
            }
            let mut text = String::new();
            let is_root = id == root;
            if e.is_complex() {
                text.push('(');
                for r in &e.records {
                    text.push_str(&r.name);
                    text.push('(');
                    params(&r.params, &map, &mut text)?;
                    text.push(')');
                }
                text.push(')');
            } else {
                // Advanced breps use advanced faces (same attributes as truck's face surfaces).
                let ename = if e.name() == "FACE_SURFACE" { "ADVANCED_FACE" } else { e.name() };
                text.push_str(ename);
                text.push('(');
                let mut ps = e.params().to_vec();
                if is_root && let Some(first) = ps.first_mut() {
                    *first = Param::Str(name.to_string());
                }
                // A face on a surface written with the opposite normal flips its sense.
                if ename == "ADVANCED_FACE"
                    && ps.get(2).and_then(Param::as_ref_id).and_then(|g| map.get(&g)).is_some_and(|g| self.flipped.contains(g))
                    && let Some(Param::Enum(sense)) = ps.get_mut(3)
                {
                    *sense = if sense == "T" { "F".into() } else { "T".into() };
                }
                params(&ps, &map, &mut text)?;
                text.push(')');
            }
            let topo = TOPOLOGY.contains(&e.name());
            let new = if topo { self.add(text) } else { self.add_shared(text) };
            map.insert(id, new);
            if matches!(e.name(), "FACE_SURFACE" | "ADVANCED_FACE") {
                self.faces_out.insert(id, new);
            }
        }
        map.get(&root).copied().ok_or_else(|| "solid not copied".into())
    }
}

/// An analytic surface found behind a surface of revolution.
#[derive(Clone, Copy)]
struct Analytic {
    kind: AnalyticKind,
    /// The analytic surface's normal is opposite to the revolution's (ISO 10303-42 normals).
    flip: bool,
}

#[derive(Clone, Copy)]
enum AnalyticKind {
    Plane {
        o: V3,
        z: V3,
        x: V3,
    },
    Cylinder {
        o: V3,
        z: V3,
        x: V3,
        r: f64,
    },
    Cone {
        o: V3,
        z: V3,
        x: V3,
        r: f64,
        angle: f64,
    },
    Sphere {
        o: V3,
        z: V3,
        x: V3,
        r: f64,
    },
    Torus {
        o: V3,
        z: V3,
        x: V3,
        big: f64,
        small: f64,
    },
    /// A torus whose tube reaches its axis (a horn torus, as a tight sweep makes): no valid
    /// TOROIDAL_SURFACE, so a rational B-spline (tube angle from the pole at the axis, then the
    /// turn from x), as other systems write it. x is the turn's parameter seam.
    HornTorus {
        o: V3,
        z: V3,
        x: V3,
        big: f64,
        small: f64,
    },
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let n = norm(a);
    (n > 1e-12 && n.is_finite()).then(|| mul(a, 1.0 / n))
}

/// Circle through three points: centre, radius, unit normal.
fn circle3(a: V3, b: V3, c: V3) -> Option<(V3, f64, V3)> {
    let (ab, ac) = (sub(b, a), sub(c, a));
    let n = cross(ab, ac);
    let n2 = dot(n, n);
    if !(n2 > 1e-24) {
        return None;
    }
    let t = add(mul(cross(n, ab), dot(ac, ac)), mul(cross(ac, n), dot(ab, ab)));
    let center = add(a, mul(t, 0.5 / n2));
    Some((center, norm(sub(a, center)), unit(n)?))
}

/// The analytic surface a surface of revolution is, if its profile is a line or a circular arc
/// in a plane through the axis.
fn analytic_revolution(ex: &p21::Exchange, e: &p21::Entity) -> Option<Analytic> {
    use crate::step_in::geom::{self, CurveGeo};
    use truck_modeling::{BoundedCurve, ParametricCurve};
    let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
    let p = e.params();
    let (ao, ad) = geom::axis1(&cx, p.get(2)?.as_ref_id()?).ok()?;
    let (oa, a) = ([ao.x, ao.y, ao.z], [ad.x, ad.y, ad.z]);
    let g = geom::curve(&cx, p.get(1)?.as_ref_id()?, 0).ok()?;
    let radial = |q: V3| {
        let v = sub(q, oa);
        sub(v, mul(a, dot(v, a)))
    };
    let on_axis = |q: V3| add(oa, mul(a, dot(sub(q, oa), a)));
    // Profile samples and tangents.
    let eval: Box<dyn Fn(f64) -> V3> = match &g {
        CurveGeo::Line { p, d } => {
            let (p, d) = ([p.x, p.y, p.z], [d.x, d.y, d.z]);
            Box::new(move |t| add(p, mul(d, t)))
        }
        CurveGeo::BSpline(c) => {
            let c = c.clone();
            let (t0, t1) = c.range_tuple();
            Box::new(move |s| {
                let q = c.subs(t0 + (t1 - t0) * s);
                [q.x, q.y, q.z]
            })
        }
        CurveGeo::Nurbs(c) => {
            let c = c.clone();
            let (t0, t1) = c.range_tuple();
            Box::new(move |s| {
                let q = c.subs(t0 + (t1 - t0) * s);
                [q.x, q.y, q.z]
            })
        }
        CurveGeo::Conic { .. } => return None,
    };
    // A sample point off the axis, its revolution normal (∂S/∂angle × ∂S/∂profile).
    let sample =
        [0.37, 0.61, 0.23, 0.79].into_iter().map(|t| (eval(t), sub(eval(t + 1e-4), eval(t - 1e-4)))).find(|(q, _)| norm(radial(*q)) > 1e-6)?;
    let (q, dq) = sample;
    let n_rev = cross(cross(a, sub(q, oa)), dq);
    let eq = unit(radial(q))?;
    let (kind, n_an) = match &g {
        CurveGeo::Line { d, .. } => {
            let d = unit([d.x, d.y, d.z])?;
            // Coplanar with the axis?
            if dot(cross(d, a), eq).abs() > 1e-9 {
                return None;
            }
            let (da, dr) = (dot(d, a), dot(d, eq));
            if dr.abs() < 1e-12 {
                let r = norm(radial(q));
                (AnalyticKind::Cylinder { o: oa, z: a, x: eq, r }, eq)
            } else if da.abs() < 1e-12 {
                (AnalyticKind::Plane { o: on_axis(q), z: a, x: eq }, a)
            } else {
                // Open the cone along +z: the radius grows that way.
                let z = if dr * da > 0.0 { a } else { mul(a, -1.0) };
                let angle = (dr.abs() / da.abs()).atan();
                let r = norm(radial(q));
                let n = sub(mul(eq, angle.cos()), mul(z, angle.sin()));
                (AnalyticKind::Cone { o: on_axis(q), z, x: eq, r, angle }, n)
            }
        }
        _ => {
            let (c, r, nc) = circle3(eval(0.0), eval(0.5), eval(1.0))?;
            // Every sample on the circle, the circle's plane through the axis.
            if (0..=8).any(|i| (norm(sub(eval(i as f64 / 8.0), c)) - r).abs() > 1e-7 * (1.0 + r)) {
                return None;
            }
            if dot(nc, a).abs() > 1e-9 || dot(sub(c, oa), nc).abs() > 1e-7 * (1.0 + r) {
                return None;
            }
            let big = norm(radial(c));
            if big < 1e-9 * (1.0 + r) {
                (AnalyticKind::Sphere { o: c, z: a, x: eq, r }, sub(q, c))
            } else {
                let ec = unit(radial(c))?;
                let o = on_axis(c);
                if (big - r).abs() <= 1e-6 * (1.0 + r) {
                    (AnalyticKind::HornTorus { o, z: a, x: mul(ec, -1.0), big: r, small: r }, sub(q, add(o, mul(eq, r))))
                } else if big < r {
                    // A spindle torus (tube crossing the axis) stays a surface of revolution.
                    return None;
                } else {
                    (AnalyticKind::Torus { o, z: a, x: ec, big, small: r }, sub(q, add(o, mul(eq, big))))
                }
            }
        }
    };
    let flip = dot(n_rev, n_an) < 0.0;
    if !dot(n_rev, n_an).is_finite() || dot(n_rev, n_an).abs() < 1e-12 {
        return None;
    }
    Some(Analytic { kind, flip })
}

/// Two analytic surfaces are the same surface (same kind, frame-independent parameters).
fn same_analytic(a: &AnalyticKind, b: &AnalyticKind) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-7 * (1.0 + x.abs().max(y.abs()));
    let same_pt = |p: V3, q: V3| norm(sub(p, q)) <= 1e-7 * (1.0 + norm(p));
    let parallel = |p: V3, q: V3| norm(cross(p, q)) <= 1e-9;
    let on_line = |p: V3, o: V3, d: V3| norm(cross(sub(p, o), d)) <= 1e-7 * (1.0 + norm(p));
    match (a, b) {
        (AnalyticKind::Cylinder { o, z, r, .. }, AnalyticKind::Cylinder { o: o2, z: z2, r: r2, .. }) => {
            parallel(*z, *z2) && on_line(*o2, *o, *z) && close(*r, *r2)
        }
        (AnalyticKind::Cone { o, z, r, angle, .. }, AnalyticKind::Cone { o: o2, z: z2, r: r2, angle: a2, .. }) => {
            // Same apex, axis direction and angle.
            let apex = |o: V3, z: V3, r: f64, an: f64| sub(o, mul(z, r / an.tan()));
            dot(*z, *z2) > 1.0 - 1e-9 && close(*angle, *a2) && same_pt(apex(*o, *z, *r, *angle), apex(*o2, *z2, *r2, *a2))
        }
        (AnalyticKind::Sphere { o, r, .. }, AnalyticKind::Sphere { o: o2, r: r2, .. }) => same_pt(*o, *o2) && close(*r, *r2),
        (AnalyticKind::Torus { o, z, big, small, .. }, AnalyticKind::Torus { o: o2, z: z2, big: b2, small: s2, .. }) => {
            same_pt(*o, *o2) && parallel(*z, *z2) && close(*big, *b2) && close(*small, *s2)
        }
        (AnalyticKind::HornTorus { o, z, big, .. }, AnalyticKind::HornTorus { o: o2, z: z2, big: b2, .. }) => {
            same_pt(*o, *o2) && parallel(*z, *z2) && close(*big, *b2)
        }
        (AnalyticKind::Plane { o, z, .. }, AnalyticKind::Plane { o: o2, z: z2, .. }) => {
            parallel(*z, *z2) && dot(sub(*o2, *o), *z).abs() <= 1e-7 * (1.0 + norm(*o))
        }
        _ => false,
    }
}

/// The outward normal sign of a face relative to its analytic surface's normal.
fn face_normal_sign(a: &AnalyticKind, z_written: V3, flip: bool, sense: bool) -> f64 {
    // Planes compare their written normal directly; the others are always outward-normal kinds.
    let base = match a {
        AnalyticKind::Plane { z, .. } => dot(*z, z_written).signum(),
        _ => 1.0,
    };
    base * if flip != sense { 1.0 } else { -1.0 }
}

/// A B-spline surface that is flat (a sheet swept along a line, as truck makes for extrusions):
/// the plane with the surface's own normal, so it can be written as a PLANE.
fn planar_bspline(ex: &p21::Exchange, e: &p21::Entity) -> Option<AnalyticKind> {
    use truck_modeling::{BoundedSurface, ParametricSurface, ParametricSurface3D};
    if !(e.has("B_SPLINE_SURFACE") || e.name() == "B_SPLINE_SURFACE_WITH_KNOTS") {
        return None;
    }
    let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
    let s = crate::step_in::geom::bspline_surface(&cx, e).ok()?;
    let (ctrl, (u0, u1), (v0, v1)) = match &s {
        truck_modeling::Surface::BSplineSurface(b) => {
            (b.control_points().iter().flatten().map(|p| [p.x, p.y, p.z]).collect::<Vec<V3>>(), b.range_tuple().0, b.range_tuple().1)
        }
        truck_modeling::Surface::NurbsSurface(n) => {
            let pts = n.non_rationalized().control_points().iter().flatten().map(|p| [p.x / p.w, p.y / p.w, p.z / p.w]).collect::<Vec<V3>>();
            let (ru, rv) = n.range_tuple();
            (pts, ru, rv)
        }
        _ => return None,
    };
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let n = s.normal(um, vm);
    let z = unit([n.x, n.y, n.z])?;
    let o = s.subs(um, vm);
    let o = [o.x, o.y, o.z];
    let du = s.uder(um, vm);
    let x = unit(sub([du.x, du.y, du.z], mul(z, dot([du.x, du.y, du.z], z))))?;
    let size = ctrl.iter().map(|p| norm(sub(*p, o))).fold(0.0, f64::max);
    // Every control point on the plane (a flat net gives a flat surface, rational or not).
    if ctrl.iter().any(|p| dot(sub(*p, o), z).abs() > 1e-9 * (1.0 + size)) {
        return None;
    }
    Some(AnalyticKind::Plane { o, z, x })
}

/// A B-spline surface that is a circular cylinder (a circle or arc swept along a line, as truck
/// makes for extruded round profiles), with its normal direction relative to the cylinder's
/// outward normal.
fn cylindrical_bspline(ex: &p21::Exchange, e: &p21::Entity) -> Option<Analytic> {
    use truck_modeling::{ParametricSurface, ParametricSurface3D};
    if !(e.has("B_SPLINE_SURFACE") || e.name() == "B_SPLINE_SURFACE_WITH_KNOTS") {
        return None;
    }
    let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
    let s = crate::step_in::geom::bspline_surface(&cx, e).ok()?;
    let (Some((u0, u1)), Some((v0, v1))) = s.try_range_tuple() else { return None };
    let at = |u: f64, v: f64| {
        let p = s.subs(u, v);
        [p.x, p.y, p.z]
    };
    // Which parameter sweeps along a line: the same translation from one end to the other at
    // every point of the profile.
    for along_v in [true, false] {
        let point = |t: f64, w: f64| if along_v { at(u0 + (u1 - u0) * t, v0 + (v1 - v0) * w) } else { at(u0 + (u1 - u0) * w, v0 + (v1 - v0) * t) };
        let prof: Vec<V3> = (0..=8).map(|k| point(k as f64 / 8.0, 0.0)).collect();
        let d = sub(point(0.0, 1.0), point(0.0, 0.0));
        let len = norm(d);
        let scale = prof.iter().map(|p| norm(sub(*p, prof[0]))).fold(len, f64::max);
        let tol = 1e-7 * (1.0 + scale);
        if len < tol
            || (0..=8).any(|k| {
                let t = k as f64 / 8.0;
                (0..=4).any(|m| norm(sub(sub(point(t, m as f64 / 4.0), point(t, 0.0)), mul(d, m as f64 / 4.0))) > tol)
            })
        {
            continue;
        }
        let (Some(&a), Some(&b), Some(&c)) = (prof.first(), prof.get(4), prof.get(8)) else { continue };
        let (centre, r, n) = circle3(a, b, c).or_else(|| circle3(a, prof[2], b))?;
        let z = unit(d)?;
        if norm(cross(n, z)) > 1e-7 || prof.iter().any(|p| (norm(sub(*p, centre)) - r).abs() > tol) {
            return None;
        }
        // The surface's own normal against the outward one, mid-face.
        let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let nn = s.normal(um, vm);
        let q = at(um, vm);
        let radial = sub(sub(q, centre), mul(z, dot(sub(q, centre), z)));
        let flip = dot([nn.x, nn.y, nn.z], radial) < 0.0;
        // The parameter seam opposite the middle of the sheet, away from its edges.
        let x = unit(mul(radial, -1.0)).or_else(|| unit(sub(a, centre)))?;
        return Some(Analytic { kind: AnalyticKind::Cylinder { o: centre, z, x, r }, flip });
    }
    None
}

/// A B-spline surface that is really a plane or a cylinder.
fn analytic_bspline(ex: &p21::Exchange, e: &p21::Entity) -> Option<Analytic> {
    planar_bspline(ex, e).map(|kind| Analytic { kind, flip: false }).or_else(|| cylindrical_bspline(ex, e))
}

/// Points along an edge's curve (its ends for a line, None for a conic it cannot sample).
fn edge_samples(ex: &p21::Exchange, edge: u64) -> Option<Vec<V3>> {
    use truck_modeling::{BoundedCurve, ParametricCurve};
    let p = ex.get(edge)?.params();
    let ends = [vertex_point(ex, p.get(1)?.as_ref_id()?)?, vertex_point(ex, p.get(2)?.as_ref_id()?)?];
    let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
    let sample = |f: &dyn Fn(f64) -> truck_modeling::Point3, (t0, t1): (f64, f64)| {
        (0..=4).map(|k| f(t0 + (t1 - t0) * k as f64 / 4.0)).map(|q| [q.x, q.y, q.z]).collect::<Vec<V3>>()
    };
    match crate::step_in::geom::curve(&cx, p.get(3)?.as_ref_id()?, 0).ok()? {
        crate::step_in::geom::CurveGeo::Line { .. } => Some(ends.to_vec()),
        crate::step_in::geom::CurveGeo::BSpline(c) => Some(sample(&|t| c.subs(t), c.range_tuple())),
        crate::step_in::geom::CurveGeo::Nurbs(c) => Some(sample(&|t| c.subs(t), c.range_tuple())),
        crate::step_in::geom::CurveGeo::Conic { .. } => None,
    }
}

fn vertex_point(ex: &p21::Exchange, v: u64) -> Option<V3> {
    let p = ex.get(ex.get(v)?.params().get(1)?.as_ref_id()?)?;
    let c = p.params().get(1)?.as_list()?;
    Some([c.first()?.as_f64()?, c.get(1)?.as_f64()?, c.get(2)?.as_f64()?])
}

/// For each horn torus revolution, a turn direction away from its faces (the B-spline's
/// parameter seam must not cross them): opposite their vertices' mean direction about the axis.
fn horn_seams(ex: &p21::Exchange) -> HashMap<u64, V3> {
    let mut out = HashMap::new();
    for fe in ex.entities.values() {
        if !matches!(fe.name(), "FACE_SURFACE" | "ADVANCED_FACE") {
            continue;
        }
        let Some(sid) = fe.params().get(2).and_then(Param::as_ref_id) else { continue };
        let Some(se) = ex.get(sid).filter(|e| e.name() == "SURFACE_OF_REVOLUTION") else { continue };
        let Some(Analytic { kind: AnalyticKind::HornTorus { o, z, .. }, .. }) = analytic_revolution(ex, se) else { continue };
        let Some(loops) = face_edges(ex, fe) else { continue };
        let mut mean = [0.0; 3];
        for (e, _) in loops.iter().flatten() {
            for k in [1, 2] {
                let Some(q) = ex.get(*e).and_then(|e| e.params().get(k)).and_then(Param::as_ref_id).and_then(|v| vertex_point(ex, v)) else {
                    continue;
                };
                let v = sub(q, o);
                if let Some(r) = unit(sub(v, mul(z, dot(v, z)))) {
                    mean = add(mean, r);
                }
            }
        }
        if let Some(m) = unit(mean) {
            out.insert(sid, mul(m, -1.0));
        }
    }
    out
}

/// Zero-length edges (truck sweeps a profile point on the axis into one) removed and their
/// vertices joined: other systems reject a pole written as an ordinary edge.
fn collapse_short_edges(ex: &mut p21::Exchange) {
    let ends = |e: &p21::Entity| Some((e.params().get(1)?.as_ref_id()?, e.params().get(2)?.as_ref_id()?));
    let mut join: HashMap<u64, u64> = HashMap::new();
    let mut dead = std::collections::HashSet::new();
    let mut ids: Vec<u64> = ex.entities.iter().filter(|(_, e)| e.name() == "EDGE_CURVE").map(|(i, _)| *i).collect();
    ids.sort();
    for id in ids {
        let Some(e) = ex.get(id) else { continue };
        let Some((a, b)) = ends(e) else { continue };
        let (Some(pa), Some(pb)) = (vertex_point(ex, a), vertex_point(ex, b)) else { continue };
        if a == b || norm(sub(pa, pb)) > 1e-6 * (1.0 + norm(pa)) {
            continue;
        }
        // The curve itself must be short too (not a whole circle between two coincident vertices).
        let short = e.params().get(3).and_then(Param::as_ref_id).and_then(|c| {
            use truck_modeling::{BoundedCurve, ParametricCurve};
            let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
            let mid = match crate::step_in::geom::curve(&cx, c, 0).ok()? {
                crate::step_in::geom::CurveGeo::Line { .. } => return Some(true),
                crate::step_in::geom::CurveGeo::BSpline(c) => c.subs((c.range_tuple().0 + c.range_tuple().1) / 2.0),
                crate::step_in::geom::CurveGeo::Nurbs(c) => c.subs((c.range_tuple().0 + c.range_tuple().1) / 2.0),
                crate::step_in::geom::CurveGeo::Conic { .. } => return Some(false),
            };
            Some(norm(sub([mid.x, mid.y, mid.z], pa)) <= 1e-6 * (1.0 + norm(pa)))
        });
        if short != Some(true) {
            continue;
        }
        let find = |join: &HashMap<u64, u64>, mut v: u64| {
            while let Some(n) = join.get(&v) {
                v = *n;
            }
            v
        };
        let (ra, rb) = (find(&join, a), find(&join, b));
        if ra != rb {
            join.insert(rb, ra);
        }
        dead.insert(id);
    }
    if dead.is_empty() {
        return;
    }
    // A loop made only of such edges (a pole loop) keeps them all.
    let oe_edge = |ex: &p21::Exchange, oe: &Param| oe.as_ref_id().and_then(|o| ex.get(o)).and_then(|o| o.params().get(3)).and_then(Param::as_ref_id);
    let loops: Vec<u64> = ex.entities.iter().filter(|(_, e)| e.name() == "EDGE_LOOP").map(|(i, _)| *i).collect();
    for l in &loops {
        let Some(list) = ex.get(*l).and_then(|e| e.params().get(1)).and_then(Param::as_list) else { continue };
        if list.iter().all(|oe| oe_edge(ex, oe).is_some_and(|e| dead.contains(&e))) {
            return;
        }
    }
    for l in loops {
        let Some(list) = ex.get(l).and_then(|e| e.params().get(1)).and_then(Param::as_list) else { continue };
        let kept: Vec<Param> = list.iter().filter(|oe| !oe_edge(ex, oe).is_some_and(|e| dead.contains(&e))).cloned().collect();
        if let Some(r) = ex.entities.get_mut(&l).and_then(|e| e.records.first_mut())
            && let Some(slot) = r.params.get_mut(1)
        {
            *slot = Param::List(kept);
        }
    }
    let find = |mut v: u64| {
        while let Some(n) = join.get(&v) {
            v = *n;
        }
        v
    };
    for e in ex.entities.values_mut() {
        if e.name() != "EDGE_CURVE" {
            continue;
        }
        if let Some(r) = e.records.first_mut() {
            for k in [1, 2] {
                if let Some(slot) = r.params.get_mut(k)
                    && let Some(v) = slot.as_ref_id()
                {
                    *slot = Param::Ref(find(v));
                }
            }
        }
    }
}

/// Oriented edges (edge id, forward) of a face's bounds.
fn face_edges(ex: &p21::Exchange, face: &p21::Entity) -> Option<Vec<Vec<(u64, bool)>>> {
    let mut loops = Vec::new();
    for b in face.params().get(1)?.as_list()? {
        let fb = ex.get(b.as_ref_id()?)?;
        let lp = ex.get(fb.params().get(1)?.as_ref_id()?)?;
        if lp.name() != "EDGE_LOOP" {
            return None;
        }
        let orient = fb.params().get(2)?.as_bool()?;
        let mut v = Vec::new();
        for oe in lp.params().get(1)?.as_list()? {
            let oe = ex.get(oe.as_ref_id()?)?;
            if oe.name() != "ORIENTED_EDGE" {
                return None;
            }
            v.push((oe.params().get(3)?.as_ref_id()?, oe.params().get(4)?.as_bool()?));
        }
        if !orient {
            v.reverse();
            v.iter_mut().for_each(|e| e.1 = !e.1);
        }
        loops.push(v);
    }
    Some(loops)
}

/// Faces that truck splits (a revolution in two or three turns) joined back into one face per
/// analytic surface, by dropping the edges between them. Faces whose union would lose all its
/// edges, or whose leftover edges do not close into loops, stay apart.
fn merge_split_faces(ex: &mut p21::Exchange, paint: &mut HashMap<u64, FaceLook>) -> HashMap<u64, V3> {
    let mut merged_surfaces = std::collections::HashSet::new();
    let mut next = ex.entities.keys().max().copied().unwrap_or(0) + 1;
    let shells: Vec<u64> = ex.entities.iter().filter(|(_, e)| matches!(e.name(), "CLOSED_SHELL" | "OPEN_SHELL")).map(|(i, _)| *i).collect();
    // Each surface's analytic form, worked out once (merged faces keep a piece's surface).
    let mut cache: HashMap<u64, Option<Analytic>> = HashMap::new();
    for sh in shells {
        for _round in 0..64 {
            let Some(faces) = ex
                .get(sh)
                .and_then(|e| e.params().get(1))
                .and_then(Param::as_list)
                .map(|l| l.iter().filter_map(Param::as_ref_id).collect::<Vec<u64>>())
            else {
                break;
            };
            // Analytic description and normal sign of each face on a revolution.
            let info: Vec<Option<(AnalyticKind, f64)>> = faces
                .iter()
                .map(|f| {
                    let fe = ex.get(*f)?;
                    if !matches!(fe.name(), "FACE_SURFACE" | "ADVANCED_FACE") {
                        return None;
                    }
                    let sid = fe.params().get(2)?.as_ref_id()?;
                    let an = (*cache.entry(sid).or_insert_with(|| {
                        let se = ex.get(sid)?;
                        match se.name() {
                            "SURFACE_OF_REVOLUTION" => analytic_revolution(ex, se),
                            // Coplanar pieces (a face split where a mirrored half was glued on).
                            "PLANE" => {
                                let cx = crate::step_in::Ctx { ex, len: 1.0, ang: 1.0, tol: 1e-6 };
                                let f = crate::step_in::geom::frame(&cx, se.params().get(1)?.as_ref_id()?).ok()?;
                                let v = |a: truck_modeling::Vector3| [a.x, a.y, a.z];
                                Some(Analytic { kind: AnalyticKind::Plane { o: [f.o.x, f.o.y, f.o.z], z: v(f.z), x: v(f.x) }, flip: false })
                            }
                            _ => analytic_bspline(ex, se).filter(|a| match a.kind {
                                // Pieces of a B-spline cylinder merge only when their curved edges
                                // are circles square to the axis (an extruded hole): crossing
                                // curves (a hole through a tube wall) read far more slowly on a
                                // cylinder than on the sheet they came with.
                                AnalyticKind::Cylinder { z, .. } => face_edges(ex, fe).is_some_and(|ls| {
                                    ls.iter().flatten().all(|(x, _)| {
                                        edge_samples(ex, *x).is_some_and(|pts| {
                                            let Some(&q0) = pts.first() else { return false };
                                            let tol = 1e-7 * (1.0 + norm(q0));
                                            pts.iter().all(|q| dot(sub(*q, q0), z).abs() <= tol)
                                                || pts.iter().all(|q| norm(cross(sub(*q, q0), z)) <= tol)
                                        })
                                    })
                                }),
                                _ => true,
                            }),
                        }
                    }))?;
                    let sense = fe.params().get(3)?.as_bool()?;
                    let z = match an.kind {
                        AnalyticKind::Plane { z, .. } => z,
                        _ => [0.0; 3],
                    };
                    let sign = face_normal_sign(&an.kind, z, an.flip, sense);
                    // Planes: normalise to one written direction for comparisons.
                    Some((an.kind, sign))
                })
                .collect();
            let mut merged = false;
            'pairs: for i in 0..faces.len() {
                for j in (i + 1)..faces.len() {
                    let (Some(Some((ka, sa))), Some(Some((kb, sb)))) = (info.get(i), info.get(j)) else { continue };
                    if !same_analytic(ka, kb) {
                        continue;
                    }
                    let plane_dir = |k: &AnalyticKind| match k {
                        AnalyticKind::Plane { z, .. } => *z,
                        _ => [0.0, 0.0, 1.0],
                    };
                    let (na, nb) = (mul(plane_dir(ka), *sa), mul(plane_dir(kb), *sb));
                    if dot(na, nb) <= 0.0 {
                        continue;
                    }
                    let (Some(&fa), Some(&fb)) = (faces.get(i), faces.get(j)) else { continue };
                    // Faces of different colours stay apart.
                    if paint.get(&fa) != paint.get(&fb) {
                        continue;
                    }
                    let (Some(ea), Some(eb)) = (ex.get(fa).and_then(|f| face_edges(ex, f)), ex.get(fb).and_then(|f| face_edges(ex, f))) else {
                        continue;
                    };
                    let all: Vec<(u64, bool)> = ea.into_iter().chain(eb).flatten().collect();
                    let shared: std::collections::HashSet<u64> =
                        all.iter().filter(|(e, d)| all.iter().any(|(e2, d2)| e2 == e && d2 != d)).map(|(e, _)| *e).collect();
                    if shared.is_empty() {
                        continue;
                    }
                    // Chain oriented edges into loops by vertices (None: they do not close).
                    let ends = |(e, d): (u64, bool)| -> Option<(u64, u64)> {
                        let p = ex.get(e)?.params();
                        let (s0, s1) = (p.get(1)?.as_ref_id()?, p.get(2)?.as_ref_id()?);
                        Some(if d { (s0, s1) } else { (s1, s0) })
                    };
                    let chain = |mut pool: Vec<(u64, bool)>| -> Option<Vec<Vec<(u64, bool)>>> {
                        let mut loops = Vec::new();
                        while let Some(first) = pool.pop() {
                            let (start, mut at) = ends(first)?;
                            let mut lp = vec![first];
                            while at != start {
                                if lp.len() > 100_000 {
                                    return None;
                                }
                                let k = pool.iter().position(|x| ends(*x).is_some_and(|(s, _)| s == at))?;
                                let x = pool.remove(k);
                                at = ends(x)?.1;
                                lp.push(x);
                            }
                            loops.push(lp);
                        }
                        Some(loops)
                    };
                    // A full turn keeps one shared edge as the face's seam, used both ways (as other
                    // systems write cylinders and cone tips); otherwise the shared edges go.
                    // (A horn torus band needs no seam: its rings meet at the pole.)
                    let mut loops = None;
                    if shared.len() >= 2 && !matches!(ka, AnalyticKind::HornTorus { .. } | AnalyticKind::Plane { .. }) {
                        // Without the shared edges: one ring (a cone tip) or two (a band). The seam
                        // runs from a vertex on the first ring to the second ring (or the apex) and
                        // back: ring A, seam, ring B, seam reversed.
                        let rest: Vec<(u64, bool)> = all.iter().copied().filter(|(e, _)| !shared.contains(e)).collect();
                        if let Some(rings) = chain(rest).filter(|r| matches!(r.len(), 1 | 2)) {
                            let starts_at = |ring: &Vec<(u64, bool)>, v: u64| ring.iter().position(|x| ends(*x).is_some_and(|(a, _)| a == v));
                            let on_axis = |v: u64| {
                                let (o, z) = match ka {
                                    AnalyticKind::Cone { o, z, .. } | AnalyticKind::Sphere { o, z, .. } => (*o, *z),
                                    _ => return false,
                                };
                                vertex_point(ex, v).is_some_and(|q| norm(cross(sub(q, o), z)) <= 1e-6 * (1.0 + norm(q)))
                            };
                            let rotated =
                                |ring: &Vec<(u64, bool)>, k: usize| ring.iter().skip(k).chain(ring.iter().take(k)).copied().collect::<Vec<_>>();
                            let mut cands: Vec<u64> = shared.iter().copied().collect();
                            cands.sort();
                            'seam: for keep in cands {
                                for &(e, d) in all.iter().filter(|(e, _)| *e == keep) {
                                    let Some((a, b)) = ends((e, d)) else { continue };
                                    let (Some(ra), Some(ka0)) = (rings.first(), rings.first().and_then(|r| starts_at(r, a))) else { continue };
                                    let mut lp = rotated(ra, ka0);
                                    lp.push((e, d));
                                    match rings.get(1) {
                                        Some(rb) => {
                                            let Some(kb) = starts_at(rb, b) else { continue };
                                            lp.extend(rotated(rb, kb));
                                        }
                                        // A cone tip (or a sphere's pole cap): the seam's far end is
                                        // the apex on the axis, off the ring.
                                        None if starts_at(ra, b).is_none() && on_axis(b) => {}
                                        None => continue,
                                    }
                                    lp.push((e, !d));
                                    loops = Some(vec![lp]);
                                    break 'seam;
                                }
                            }
                        }
                    }
                    let planar = matches!(ka, AnalyticKind::Plane { .. });
                    if loops.is_none() && planar {
                        // A plane needs no seam: an annulus keeps its two rings, a face with a hole
                        // its hole. The largest loop goes first (the outer bound).
                        let rest: Vec<(u64, bool)> = all.iter().copied().filter(|(e, _)| !shared.contains(e)).collect();
                        loops = chain(rest).map(|mut l| {
                            let size = |lp: &Vec<(u64, bool)>| {
                                let pts: Vec<V3> = lp.iter().filter_map(|x| ends(*x).and_then(|(a, _)| vertex_point(ex, a))).collect();
                                let mut lo = [f64::INFINITY; 3];
                                let mut hi = [f64::NEG_INFINITY; 3];
                                for p in &pts {
                                    for k in 0..3 {
                                        lo[k] = lo[k].min(p[k]);
                                        hi[k] = hi[k].max(p[k]);
                                    }
                                }
                                norm(sub(hi, lo))
                            };
                            l.sort_by(|a, b| size(b).total_cmp(&size(a)));
                            l
                        });
                    }
                    if loops.is_none() && !planar {
                        let rest: Vec<(u64, bool)> = all.iter().copied().filter(|(e, _)| !shared.contains(e)).collect();
                        loops = chain(rest)
                            .map(|mut l| {
                                // Rings meeting at a vertex (both ends of a horn torus band meet at
                                // its pole) form one figure-eight loop through it.
                                while l.len() > 1 {
                                    let Some(second) = l.pop() else { break };
                                    let at = l.iter().enumerate().find_map(|(li, lp)| {
                                        lp.iter().enumerate().find_map(|(k, x)| {
                                            let v = ends(*x)?.1;
                                            let s = second.iter().position(|y| ends(*y).is_some_and(|(a, _)| a == v))?;
                                            Some((li, k, s))
                                        })
                                    });
                                    let Some((li, k, s)) = at else {
                                        l.push(second);
                                        break;
                                    };
                                    let mut second = second;
                                    second.rotate_left(s);
                                    if let Some(lp) = l.get_mut(li) {
                                        for (n, x) in second.into_iter().enumerate() {
                                            lp.insert(k + 1 + n, x);
                                        }
                                    }
                                }
                                l
                            })
                            .filter(|l| l.len() == 1);
                    }
                    let Some(loops) = loops else { continue };
                    // A loop of seams only (a whole sphere) bounds nothing: keep the faces apart.
                    if loops.iter().all(|l| l.iter().all(|(e, _)| l.iter().filter(|(e2, _)| e2 == e).count() > 1)) {
                        continue;
                    }
                    // Write the merged face.
                    let mut bounds = Vec::new();
                    for lp in loops {
                        let mut oes = Vec::new();
                        for (e, d) in lp {
                            let id = next;
                            next += 1;
                            ex.entities.insert(
                                id,
                                p21::Entity {
                                    records: vec![p21::Record {
                                        name: "ORIENTED_EDGE".into(),
                                        params: vec![
                                            Param::Str(String::new()),
                                            Param::Derived,
                                            Param::Derived,
                                            Param::Ref(e),
                                            Param::Enum(if d { "T" } else { "F" }.into()),
                                        ],
                                    }],
                                },
                            );
                            oes.push(Param::Ref(id));
                        }
                        let lid = next;
                        next += 1;
                        ex.entities.insert(
                            lid,
                            p21::Entity {
                                records: vec![p21::Record { name: "EDGE_LOOP".into(), params: vec![Param::Str(String::new()), Param::List(oes)] }],
                            },
                        );
                        let bid = next;
                        next += 1;
                        ex.entities.insert(
                            bid,
                            p21::Entity {
                                records: vec![p21::Record {
                                    name: "FACE_BOUND".into(),
                                    params: vec![Param::Str(String::new()), Param::Ref(lid), Param::Enum("T".into())],
                                }],
                            },
                        );
                        bounds.push(Param::Ref(bid));
                    }
                    let Some(old) = ex.get(fa).cloned() else { continue };
                    let mut params = old.params().to_vec();
                    if let Some(slot) = params.get_mut(1) {
                        *slot = Param::List(bounds);
                    }
                    let nid = next;
                    next += 1;
                    if let Some(sid) = params.get(2).and_then(Param::as_ref_id) {
                        merged_surfaces.insert(sid);
                    }
                    ex.entities.insert(nid, p21::Entity { records: vec![p21::Record { name: old.name().to_string(), params }] });
                    if let Some(look) = paint.remove(&fa) {
                        paint.remove(&fb);
                        paint.insert(nid, look);
                    }
                    let new_list: Vec<Param> = faces.iter().filter(|f| **f != fb).map(|f| Param::Ref(if *f == fa { nid } else { *f })).collect();
                    if let Some(she) = ex.entities.get_mut(&sh)
                        && let Some(r) = she.records.first_mut()
                        && let Some(slot) = r.params.get_mut(1)
                    {
                        *slot = Param::List(new_list);
                    }
                    merged = true;
                    break 'pairs;
                }
            }
            if !merged {
                break;
            }
        }
    }
    // B-spline cylinder sheets under merged faces are written as cylinders (the sheet covers only
    // its own piece), with the parameter seam on the face's seam edge for a full turn, else
    // opposite the face's middle. (Faces replaced by merges stay in the exchange unreferenced:
    // only the shells' faces count.)
    let live: Vec<u64> = ex
        .entities
        .values()
        .filter_map(|e| if matches!(e.name(), "CLOSED_SHELL" | "OPEN_SHELL") { e.params().get(1).and_then(Param::as_list) } else { None })
        .flatten()
        .filter_map(Param::as_ref_id)
        .collect();
    let mut out = HashMap::new();
    for e in live.iter().filter_map(|f| ex.get(*f)) {
        let Some(sid) = e.params().get(2).and_then(Param::as_ref_id) else { continue };
        if !merged_surfaces.contains(&sid) || out.contains_key(&sid) {
            continue;
        }
        let Some(Analytic { kind: AnalyticKind::Cylinder { o, z, .. }, .. }) = ex.get(sid).and_then(|se| cylindrical_bspline(ex, se)) else {
            continue;
        };
        let Some(loops) = face_edges(ex, e) else { continue };
        let radial = |q: V3| {
            let v = sub(q, o);
            sub(v, mul(z, dot(v, z)))
        };
        let seam = loops
            .iter()
            .flat_map(|l| l.iter().map(move |(x, _)| (l, *x)))
            .find(|(l, x)| l.iter().filter(|(y, _)| y == x).count() > 1)
            .map(|(_, x)| x);
        let x = match seam.and_then(|edge| ex.get(edge)?.params().get(1)?.as_ref_id()).and_then(|v| vertex_point(ex, v)) {
            Some(q) => unit(radial(q)),
            None => {
                let mean = loops
                    .iter()
                    .flatten()
                    .filter_map(|(edge, _)| ex.get(*edge)?.params().get(1)?.as_ref_id())
                    .filter_map(|v| vertex_point(ex, v))
                    .filter_map(|q| unit(radial(q)))
                    .fold([0.0; 3], add);
                unit(mul(mean, -1.0))
            }
        };
        if let Some(x) = x {
            out.insert(sid, x);
        }
    }
    out
}

/// The B-rep entities of one body as truck writes them, parsed, and the surfaces of faces
/// merged from pieces.
fn brep_exchange(b: &Body, merge: bool) -> Result<(p21::Exchange, HashMap<u64, V3>, HashMap<u64, FaceLook>)> {
    let text = crate::step::truck_step(&[b], "SolveCraft")?;
    let mut ex = p21::parse(&text).map_err(|e| KernelError::Failed(format!("STEP export: {e}")))?;
    collapse_short_edges(&mut ex);
    // Face colours by exchange face: truck writes each shell's faces in the body's face order.
    let mut paint = HashMap::new();
    if let Some(p) = b.paint()
        && !p.faces.is_empty()
    {
        let mut shells: Vec<u64> = ex.entities.iter().filter(|(_, e)| matches!(e.name(), "CLOSED_SHELL" | "OPEN_SHELL")).map(|(i, _)| *i).collect();
        shells.sort();
        let faces: Vec<u64> = shells.iter().filter_map(|s| ex.get(*s)?.params().get(1)?.as_list()).flatten().filter_map(Param::as_ref_id).collect();
        for f in &p.faces {
            if let Some(id) = faces.get(f.face) {
                paint.insert(*id, (f.color, f.opacity));
            }
        }
    }
    let before = shell_face_count(&ex);
    let merged = if merge { merge_split_faces(&mut ex, &mut paint) } else { Default::default() };
    MERGES.with(|m| m.set(m.get() + before.saturating_sub(shell_face_count(&ex))));
    Ok((ex, merged, paint))
}

/// Faces in the exchange's shells (fewer after merging).
fn shell_face_count(ex: &p21::Exchange) -> usize {
    ex.entities
        .values()
        .filter(|e| matches!(e.name(), "CLOSED_SHELL" | "OPEN_SHELL"))
        .filter_map(|e| e.params().get(1).and_then(Param::as_list))
        .map(<[Param]>::len)
        .sum()
}

/// A face's own colour (0..1) and opacity.
type FaceLook = ([f32; 3], f32);

fn rigid_frame(m: &[[f64; 4]; 4]) -> Result<([f64; 3], [f64; 3], [f64; 3])> {
    let x = [m[0][0], m[0][1], m[0][2]];
    let y = [m[1][0], m[1][1], m[1][2]];
    let z = [m[2][0], m[2][1], m[2][2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let ok = (dot(x, x) - 1.0).abs() < 1e-6 && (dot(y, y) - 1.0).abs() < 1e-6 && dot(x, y).abs() < 1e-6 && dot(cross, z) > 0.999_999;
    if !ok || m.iter().flatten().any(|v| !v.is_finite()) {
        return Err(KernelError::Invalid("component placements must be rigid (rotation and translation) to export as STEP".into()));
    }
    Ok(([m[3][0], m[3][1], m[3][2]], z, x))
}

/// STEP text (AP242) for products with bodies and assembly placements; `root` is the top product.
pub fn step_export_products(products: &[ExportProduct], root: usize, header: &StepHeader) -> Result<String> {
    MERGES.with(|m| m.set(0));
    let text = products_text(products, root, header, true)?;
    // Faces merged per surface must read back as cleanly as the pieces they came from (a merge
    // can leave a loop the reader cannot lay out); otherwise the pieces are written. Without a
    // merge there is nothing to check.
    let clean = |t: &str| crate::step_in::step_import(t).is_ok_and(|i| i.warnings.is_empty());
    if MERGES.with(|m| m.get()) > 0 && !clean(&text) {
        let plain = products_text(products, root, header, false)?;
        if clean(&plain) {
            return Ok(plain);
        }
    }
    Ok(text)
}

thread_local! {
    /// Faces merged while writing the current export.
    static MERGES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn products_text(products: &[ExportProduct], root: usize, header: &StepHeader, merge: bool) -> Result<String> {
    if products.is_empty() || root >= products.len() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    if products.iter().all(|p| p.bodies.is_empty()) {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    for p in products {
        for b in &p.bodies {
            b.body.require_brep("STEP export")?;
        }
        if p.children.iter().any(|(c, _, _)| *c >= products.len()) {
            return Err(KernelError::Invalid("bad component reference".into()));
        }
    }
    // No product may contain itself.
    fn cyclic(products: &[ExportProduct], p: usize, stack: &mut Vec<usize>) -> bool {
        if stack.contains(&p) || stack.len() > 64 {
            return true;
        }
        stack.push(p);
        let bad = products.get(p).is_some_and(|x| x.children.iter().any(|(c, _, _)| cyclic(products, *c, stack)));
        stack.pop();
        bad
    }
    if cyclic(products, root, &mut Vec::new()) {
        return Err(KernelError::Invalid("the component tree is recursive".into()));
    }

    let mut o = Out::default();
    let app = o.add("APPLICATION_CONTEXT('managed model based 3d engineering')");
    o.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','ap242_managed_model_based_3d_engineering',2014,#{app})"));
    let pctx = o.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdctx = o.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let mm = o.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let rad = o.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
    let sr = o.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
    let unc = o.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{mm},'distance_accuracy_value','confusion accuracy')"));
    let ctx = o.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    let origin = o.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);

    // Products (only those reachable from the root).
    let mut reach = vec![false; products.len()];
    let mut todo = vec![root];
    while let Some(p) = todo.pop() {
        if let Some(r) = reach.get_mut(p)
            && !*r
        {
            *r = true;
            if let Some(x) = products.get(p) {
                todo.extend(x.children.iter().map(|(c, _, _)| *c));
            }
        }
    }
    let mut pd = vec![0u64; products.len()];
    let mut shape_rep = vec![0u64; products.len()];
    let mut product_ids = Vec::new();
    let mut styled = Vec::new();
    let mut child_items: Vec<Vec<u64>> = vec![Vec::new(); products.len()];
    // Placements of children inside each parent, made before the parent's representation.
    let mut placements: Vec<Vec<u64>> = vec![Vec::new(); products.len()];
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        for (_, m, _) in &p.children {
            let (t, z, x) = rigid_frame(m)?;
            let (pt, dz, dx) = (o.point(t), o.direction(z), o.direction(x));
            // Its own entity: it is an item of the parent's representation.
            let a = o.add(format!("AXIS2_PLACEMENT_3D('',#{pt},#{dz},#{dx})"));
            if let Some(v) = placements.get_mut(pi) {
                v.push(a);
            }
            if let Some(v) = child_items.get_mut(pi) {
                v.push(a);
            }
        }
    }
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        let name = if p.name.trim().is_empty() { "Part".to_string() } else { p.name.clone() };
        let prod = o.add(format!("PRODUCT({},{},'',(#{pctx}))", string(&name), string(&name)));
        product_ids.push(prod);
        let pdf = o.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let d = o.add(format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdctx})"));
        let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{d})"));
        let mut items = vec![origin];
        items.extend(child_items.get(pi).cloned().unwrap_or_default());
        let item_list = items.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        let srep = o.add(format!("SHAPE_REPRESENTATION({},({item_list}),#{ctx})", string(&name)));
        o.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{srep})"));
        if let Some(slot) = pd.get_mut(pi) {
            *slot = d;
        }
        if let Some(slot) = shape_rep.get_mut(pi) {
            *slot = srep;
        }
        if !p.bodies.is_empty() {
            let mut solids = Vec::new();
            for b in &p.bodies {
                let (ex, merged, face_paint) = brep_exchange(b.body, merge)?;
                o.merged = merged;
                o.faces_out.clear();
                let mut roots: Vec<u64> =
                    ex.entities.iter().filter(|(_, e)| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")).map(|(i, _)| *i).collect();
                roots.sort();
                for r in roots {
                    let id = o.copy_solid(&ex, r, &b.name).map_err(|e| KernelError::Failed(format!("STEP export: {e}")))?;
                    solids.push(id);
                    let opacity = b.body.paint().map(|p| p.opacity).unwrap_or(1.0);
                    if let Some(c) = b.color {
                        styled.push(style(&mut o, id, c, opacity));
                    }
                }
                // Faces with colours of their own.
                let mut painted: Vec<(u64, ([f32; 3], f32))> =
                    face_paint.into_iter().filter_map(|(f, look)| Some((*o.faces_out.get(&f)?, look))).collect();
                painted.sort_by_key(|(f, _)| *f);
                for (f, (c, op)) in painted {
                    styled.push(style(&mut o, f, c, op));
                }
            }
            let list = solids.iter().chain(std::iter::once(&origin)).map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
            let absr = o.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION({},({list}),#{ctx})", string(&name)));
            o.add(format!("SHAPE_REPRESENTATION_RELATIONSHIP('','',#{srep},#{absr})"));
        }
    }
    if !product_ids.is_empty() {
        let list = product_ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        o.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part','',({list}))"));
    }
    // Assembly occurrences.
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        for (k, (ci, _, oname)) in p.children.iter().enumerate() {
            let (Some(&parent_pd), Some(&child_pd), Some(&parent_sr), Some(&child_sr), Some(&target)) =
                (pd.get(pi), pd.get(*ci), shape_rep.get(pi), shape_rep.get(*ci), placements.get(pi).and_then(|v| v.get(k)))
            else {
                continue;
            };
            let nm = if oname.trim().is_empty() {
                format!("{}:{}", products.get(*ci).map(|c| c.name.as_str()).unwrap_or(""), k + 1)
            } else {
                oname.clone()
            };
            let nauo = o.add(format!("NEXT_ASSEMBLY_USAGE_OCCURRENCE({},{},'',#{parent_pd},#{child_pd},$)", string(&nm), string(&nm)));
            let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{nauo})"));
            let idt = o.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{origin},#{target})"));
            let rr = o.add(format!(
                "(REPRESENTATION_RELATIONSHIP('','',#{child_sr},#{parent_sr})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt})SHAPE_REPRESENTATION_RELATIONSHIP())"
            ));
            o.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{pds})"));
        }
    }
    if !styled.is_empty() {
        let list = styled.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        o.add(format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({list}),#{ctx})"));
    }

    let mut s = String::new();
    s.push_str("ISO-10303-21;\nHEADER;\n");
    s.push_str("FILE_DESCRIPTION(('SolveCraft model'),'2;1');\n");
    let _ = writeln!(
        s,
        "FILE_NAME({},{},({}),({}),'SolveCraft','SolveCraft','');",
        string(&header.file_name),
        string(&header.time_stamp),
        string(&header.author),
        string(&header.organization)
    );
    s.push_str("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n");
    for (i, l) in o.lines.iter().enumerate() {
        let _ = writeln!(s, "#{}={l};", i + 1);
    }
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(s)
}

/// A surface colour for a solid (STYLED_ITEM chain).
fn style(o: &mut Out, item: u64, c: [f32; 3], opacity: f32) -> u64 {
    let rgb = o.add_shared(format!("COLOUR_RGB('',{},{},{})", real(c[0] as f64), real(c[1] as f64), real(c[2] as f64)));
    let fasc = o.add_shared(format!("FILL_AREA_STYLE_COLOUR('',#{rgb})"));
    let fas = o.add_shared(format!("FILL_AREA_STYLE('',(#{fasc}))"));
    let ssfa = o.add_shared(format!("SURFACE_STYLE_FILL_AREA(#{fas})"));
    // See-through looks add a transparency (1 = invisible) to the side style.
    let sides = if opacity < 1.0 {
        let tr = o.add_shared(format!("SURFACE_STYLE_TRANSPARENT({})", real((1.0 - opacity as f64).clamp(0.0, 1.0))));
        format!("#{ssfa},#{tr}")
    } else {
        format!("#{ssfa}")
    };
    let sss = o.add_shared(format!("SURFACE_SIDE_STYLE('',({sides}))"));
    let ssu = o.add_shared(format!("SURFACE_STYLE_USAGE(.BOTH.,#{sss})"));
    let psa = o.add_shared(format!("PRESENTATION_STYLE_ASSIGNMENT((#{ssu}))"));
    o.add(format!("STYLED_ITEM('color',(#{psa}),#{item})"))
}

/// STEP text for bodies of one part (names `Body1`…).
pub fn step_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    let name = if header.file_name.is_empty() {
        "Part".to_string()
    } else {
        header.file_name.trim_end_matches(".step").trim_end_matches(".stp").to_string()
    };
    let p = ExportProduct {
        name,
        bodies: bodies.iter().map(|b| ExportBody { name: b.name.clone(), body: b.body, color: b.color }).collect(),
        children: Vec::new(),
    };
    step_export_products(&[p], 0, header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_strings() {
        assert_eq!(real(1.0), "1.0");
        assert_eq!(real(1e-6), "1.E-6");
        assert_eq!(real(-2.5e20), "-2.5E20");
        assert_eq!(real(3.0e100), "3.E100");
        assert_eq!(string("it's"), "'it''s'");
        assert_eq!(string("é"), "'\\X2\\00E9\\X0\\'");
        let ex = p21::parse(&format!("ISO-10303-21;DATA;#1=A({},{});ENDSEC;END-ISO-10303-21;", string("a'b\\é"), real(1e-6))).unwrap();
        let p = ex.get(1).unwrap().params();
        assert_eq!(p[0].as_str(), Some("a'b\\é"));
        assert_eq!(p[1].as_f64(), Some(1e-6));
    }

    #[test]
    fn assembly_names_colours_round_trip() {
        use crate::{box_solid, cylinder, measure, step_import, step_validate};
        let bx = box_solid(solvecraft_geom::Vec3::ZERO, solvecraft_geom::Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let pin = cylinder(solvecraft_geom::Vec3::ZERO, solvecraft_geom::Vec3::Z, 2.0, 8.0).unwrap();
        let rot90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, 0.0, 0.0, 1.0]];
        let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 5.0, 1.0]];
        let products = vec![
            ExportProduct {
                name: "Root Ä".into(),
                bodies: vec![ExportBody { name: "Base".into(), body: &bx, color: Some([1.0, 0.0, 0.0]) }],
                children: vec![(1, shift, "Pin:1".into()), (1, rot90, "Pin:2".into())],
            },
            ExportProduct { name: "Pin".into(), bodies: vec![ExportBody { name: "Shaft".into(), body: &pin, color: None }], children: vec![] },
        ];
        let text = step_export_products(&products, 0, &StepHeader { file_name: "a.step".into(), ..Default::default() }).unwrap();
        step_validate(&text).unwrap();
        assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE").count(), 2);
        // Shared geometry: the unit Z direction is written once.
        assert_eq!(text.matches("DIRECTION('',(0.0,0.0,1.0))").count(), 1);
        let imp = step_import(&text).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        assert_eq!(imp.bodies.len(), 3);
        let names: Vec<&str> = imp.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Base", "Shaft", "Shaft"]);
        assert_eq!(imp.bodies[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(imp.tree[0].name, "Root Ä");
        assert_eq!(imp.tree[0].children.len(), 2);
        let m: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
        assert!((m[1].bbox.min.z - 5.0).abs() < 1e-6 && (m[1].bbox.max.z - 13.0).abs() < 1e-6);
        // Rotated 90° about Z and moved 50 along X: centre at (50, 0).
        assert!(m[2].centroid.dist(solvecraft_geom::Vec3::new(50.0, 0.0, 4.0)) < 1e-3, "{:?}", m[2].centroid);
        // Recursion and bad placements are errors.
        let cyc = vec![ExportProduct {
            name: "A".into(),
            bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }],
            children: vec![(0, shift, String::new())],
        }];
        assert!(step_export_products(&cyc, 0, &StepHeader::default()).is_err());
        let scaled = [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]];
        let bad = vec![
            ExportProduct { name: "A".into(), bodies: vec![], children: vec![(1, scaled, String::new())] },
            ExportProduct { name: "B".into(), bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }], children: vec![] },
        ];
        assert!(step_export_products(&bad, 0, &StepHeader::default()).is_err());
    }
}
